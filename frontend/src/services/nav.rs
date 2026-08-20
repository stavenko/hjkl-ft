//! Нажатия на пункты нижнего меню — как СОБЫТИЕ, а не только переход.
//!
//! Зачем. Пункт меню ведёт на тот же адрес, на котором человек уже стоит, и
//! роутер в этом случае не делает ничего: страница не перемонтируется, её
//! состояние остаётся прежним. А человек, ткнув в «Главную», ждёт главную — не
//! раскрытую поверх неё панель веса, которую он открыл минуту назад. С дневником
//! так же: пролистав к позавчерашнему дню, он жмёт «Дневник» и ждёт сегодня.
//!
//! Поэтому пункты меню, кроме перехода, дёргают счётчик. Страница подписывается на
//! него и сбрасывает своё состояние: дашборд закрывает оверлей, дневник
//! возвращается на сегодня. Счётчик, а не флаг: важен сам факт нажатия, в том
//! числе повторного, и его не надо гасить обратно.

use std::cell::RefCell;

use leptos::*;

thread_local! {
    static HOME: RefCell<Option<RwSignal<u32>>> = const { RefCell::new(None) };
    static DIARY: RefCell<Option<RwSignal<u32>>> = const { RefCell::new(None) };
}

/// Завести сигналы в корневой области видимости. Вызывается один раз из `main()`
/// внутри рантайма Leptos — как `stories::init`.
pub fn init() {
    HOME.with(|c| *c.borrow_mut() = Some(create_rw_signal(0)));
    DIARY.with(|c| *c.borrow_mut() = Some(create_rw_signal(0)));
}

fn signal(cell: &'static std::thread::LocalKey<RefCell<Option<RwSignal<u32>>>>) -> RwSignal<u32> {
    cell.with(|c| *c.borrow()).expect("nav::init() must run first")
}

/// Счётчик нажатий на «Главную». Дашборд следит за ним и закрывает открытое.
pub fn home_taps() -> RwSignal<u32> {
    signal(&HOME)
}

/// Счётчик нажатий на «Дневник». Страница дневника возвращается на сегодня.
pub fn diary_taps() -> RwSignal<u32> {
    signal(&DIARY)
}

pub fn tap_home() {
    home_taps().update(|n| *n += 1);
}

pub fn tap_diary() {
    diary_taps().update(|n| *n += 1);
}
