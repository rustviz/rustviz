#[derive(Clone, Copy)]
enum Number { One(u32), Unit(u32), Onerous(u32) }

fn number() -> u32 { 1 }

fn constructors() {
    let one = Number::One(number());
    let unit = Number::Unit(number());
    let onerous = Number::Onerous(number());
    let ok: Result<u32, u32> = Ok(number());
    let err: Result<u32, u32> = Err(number());
    let _ = (one, unit, onerous, ok, err);
}

use std::option::Option::Some as One;

fn direct_constructors(n: u32) {
    let direct = Number::One(2);
    if let Some(inside) = One(n) { let _ = inside; }
    let _ = direct;
}
