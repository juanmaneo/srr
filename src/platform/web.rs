//! The browser glue: the WebSocket to the relay server (`srr-server`), served from the same
//! host as the page, on `/ws`, and the audio unlock (browsers only play sound started from a
//! user gesture).

use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::rc::Rc;

use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::Closure;
use web_sys::{BinaryType, Event, MessageEvent, WebSocket};

use super::sound::{Sound, report};
use crate::audio::MusicControl;
use crate::front::{Game, SoundState};
use crate::game::storage;
use crate::game::{KeyBindings, MatchConfig};
use crate::net::{Client, INPUT_DELAY};

/// The DOM events that count as a user gesture for the autoplay policy.
const GESTURES: [&str; 4] = ["keydown", "pointerdown", "pointerup", "touchend"];

/// Starts the music inside the first user gesture, and resumes it on later ones if the browser
/// suspended it.
///
/// The engines see keys inside their own frame callbacks (`requestAnimationFrame`), which are
/// not user gestures: an `AudioContext` created or resumed there may stay suspended, silently.
/// So the audio is driven from capture-phase listeners on the window, which run inside the
/// browser's key / pointer / touch event handlers, before the engine's own listeners.
pub(super) struct AudioUnlock {
    sound: Rc<RefCell<Option<Sound>>>,
    failed: Rc<Cell<bool>>,
    // Must live as long as the listeners.
    _on_gesture: Closure<dyn FnMut(Event)>,
}

impl AudioUnlock {
    /// Listens for gestures; `None` (after a console message) without a window.
    pub(super) fn install(control: MusicControl) -> Option<Self> {
        let Some(window) = web_sys::window() else {
            report("srr: no window: no music");
            return None;
        };
        let sound = Rc::new(RefCell::new(None::<Sound>));
        let failed = Rc::new(Cell::new(false));

        let slot = Rc::clone(&sound);
        let failure = Rc::clone(&failed);
        let on_gesture = Closure::<dyn FnMut(Event)>::new(move |_: Event| {
            let mut slot = slot.borrow_mut();
            match slot.as_ref() {
                Some(playing) => playing.resume(),
                None if !failure.get() => {
                    *slot = Sound::start(control.clone());
                    failure.set(slot.is_none());
                }
                None => {}
            }
        });
        for gesture in GESTURES {
            if window
                .add_event_listener_with_callback_and_bool(
                    gesture,
                    on_gesture.as_ref().unchecked_ref(),
                    true,
                )
                .is_err()
            {
                report(&format!(
                    "srr: cannot listen to {gesture} events for the music"
                ));
            }
        }
        Some(AudioUnlock {
            sound,
            failed,
            _on_gesture: on_gesture,
        })
    }

    /// Whether the music plays now.
    pub(super) fn state(&self) -> SoundState {
        match self.sound.borrow().as_ref() {
            Some(sound) if sound.is_running() => SoundState::Playing,
            // Created or resumed, but the browser still holds it back: needs another gesture.
            Some(_) => SoundState::Waiting,
            None if self.failed.get() => SoundState::Unavailable,
            None => SoundState::Waiting,
        }
    }
}

enum NetEvent {
    Message(Vec<u8>),
    Closed,
}

/// The open socket and the events its callbacks queued. Not `Send`: lives on the main thread.
pub(super) struct Connection {
    socket: WebSocket,
    inbox: Rc<RefCell<VecDeque<NetEvent>>>,
    // The callbacks must live as long as the socket.
    _on_message: Closure<dyn FnMut(MessageEvent)>,
    _on_close: Closure<dyn FnMut(Event)>,
}

impl Connection {
    fn open(url: &str) -> Option<Self> {
        let socket = WebSocket::new(url).ok()?;
        socket.set_binary_type(BinaryType::Arraybuffer);
        let inbox = Rc::new(RefCell::new(VecDeque::new()));

        let messages = Rc::clone(&inbox);
        let on_message = Closure::<dyn FnMut(MessageEvent)>::new(move |event: MessageEvent| {
            if let Ok(buffer) = event.data().dyn_into::<js_sys::ArrayBuffer>() {
                let bytes = js_sys::Uint8Array::new(&buffer).to_vec();
                messages.borrow_mut().push_back(NetEvent::Message(bytes));
            }
        });
        socket.set_onmessage(Some(on_message.as_ref().unchecked_ref()));

        let closes = Rc::clone(&inbox);
        let on_close = Closure::<dyn FnMut(Event)>::new(move |_: Event| {
            closes.borrow_mut().push_back(NetEvent::Closed);
        });
        socket.set_onclose(Some(on_close.as_ref().unchecked_ref()));
        socket.set_onerror(Some(on_close.as_ref().unchecked_ref()));

        Some(Connection {
            socket,
            inbox,
            _on_message: on_message,
            _on_close: on_close,
        })
    }

    /// Hands everything the server sent since the last tick to the game's client.
    pub(super) fn receive(&self, game: &mut Game) {
        let events: Vec<NetEvent> = self.inbox.borrow_mut().drain(..).collect();
        for event in events {
            match event {
                NetEvent::Message(bytes) => game.client_mut().receive(&bytes),
                NetEvent::Closed => game.client_mut().disconnected(),
            }
        }
    }

    /// Sends one frame, if the socket is open.
    pub(super) fn send(&self, frame: &[u8]) {
        if self.socket.ready_state() == WebSocket::OPEN {
            let _ = self.socket.send_with_u8_array(frame);
        }
    }
}

/// Whether the page URL asks for practice mode (`?practice`).
pub(super) fn practice_requested() -> bool {
    web_sys::window()
        .and_then(|window| window.location().search().ok())
        .is_some_and(|query| query.contains("practice"))
}

/// The key of the saved keyboard layout in the browser's local storage.
const BINDINGS_KEY: &str = "srr-bindings";

/// The keyboard layout saved by [`save_bindings`], if the browser has one and it still loads
/// (see [`storage::load`]).
pub(super) fn saved_bindings() -> Option<KeyBindings> {
    web_sys::window()?
        .local_storage()
        .ok()
        .flatten()?
        .get_item(BINDINGS_KEY)
        .ok()
        .flatten()
        .and_then(|saved| storage::load(&saved))
}

/// Saves `bindings` in the browser's local storage, so the player keeps them next time; the
/// page's own script calls it (from `Shell::tick`), and a hidden element reports when it
/// cannot.
pub(super) fn save_bindings(bindings: &KeyBindings) {
    let Some(storage) = web_sys::window().and_then(|w| w.local_storage().ok().flatten()) else {
        return;
    };
    let _ = storage.set_item(BINDINGS_KEY, &storage::store(bindings));
}

/// `ws://<host of the page>/ws` (`wss://` when the page is served over HTTPS).
fn server_url() -> Option<String> {
    let location = web_sys::window()?.location();
    let scheme = if location.protocol().ok()? == "https:" {
        "wss"
    } else {
        "ws"
    };
    Some(format!("{scheme}://{}/ws", location.host().ok()?))
}

/// The online client and its connection (`None`, with a disconnected client, on failure).
pub(super) fn connect() -> (Client, Option<Connection>) {
    let mut client = Client::online(MatchConfig::default(), INPUT_DELAY);
    let connection = server_url().and_then(|url| Connection::open(&url));
    if connection.is_none() {
        client.disconnected();
    }
    (client, connection)
}
