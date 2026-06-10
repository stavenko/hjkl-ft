use leptos::*;
use std::cell::RefCell;

#[derive(Clone, Debug, PartialEq)]
pub enum NotifyLevel {
    Info,
    Success,
    Warning,
    Error,
    Urgent,
}

#[derive(Clone, Debug)]
pub struct Notification {
    pub id: u32,
    pub message: String,
    pub level: NotifyLevel,
    pub auto_dismiss_ms: Option<u32>,
}

thread_local! {
    static NEXT_ID: RefCell<u32> = const { RefCell::new(1) };
    static SIGNAL: RefCell<Option<RwSignal<Vec<Notification>>>> = const { RefCell::new(None) };
}

pub fn init() -> RwSignal<Vec<Notification>> {
    let signal = create_rw_signal(Vec::<Notification>::new());
    SIGNAL.with(|s| *s.borrow_mut() = Some(signal));
    signal
}

pub fn get_signal() -> Option<RwSignal<Vec<Notification>>> {
    SIGNAL.with(|s| *s.borrow())
}

fn next_id() -> u32 {
    NEXT_ID.with(|id| {
        let mut id = id.borrow_mut();
        let current = *id;
        *id += 1;
        current
    })
}

fn push(message: String, level: NotifyLevel, auto_dismiss_ms: Option<u32>) {
    let Some(signal) = get_signal() else { return };
    let id = next_id();
    let notif = Notification { id, message, level, auto_dismiss_ms };

    signal.update(|list| list.push(notif));

    if let Some(ms) = auto_dismiss_ms {
        spawn_local(async move {
            let promise = js_sys::Promise::new(&mut |resolve, _| {
                let window = web_sys::window().expect("no window");
                let _ = window.set_timeout_with_callback_and_timeout_and_arguments_0(&resolve, ms as i32);
            });
            let _ = wasm_bindgen_futures::JsFuture::from(promise).await;
            dismiss(id);
        });
    }
}

pub fn dismiss(id: u32) {
    let Some(signal) = get_signal() else { return };
    signal.update(|list| list.retain(|n| n.id != id));
}

pub fn info(message: &str) {
    push(message.to_string(), NotifyLevel::Info, Some(4000));
}

pub fn success(message: &str) {
    push(message.to_string(), NotifyLevel::Success, Some(3000));
}

pub fn warning(message: &str) {
    push(message.to_string(), NotifyLevel::Warning, Some(6000));
}

pub fn error(message: &str) {
    push(message.to_string(), NotifyLevel::Error, Some(8000));
}

pub fn urgent(message: &str) {
    push(message.to_string(), NotifyLevel::Urgent, None);
}
