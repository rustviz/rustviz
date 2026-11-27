#[derive(Debug)] enum Trace { A, B, Spare, Maybe, Early }
impl Drop for Trace {
    fn drop(&mut self) { println!("drop {self:?}"); }
}

struct Pair { a: Trace, b: Trace }

fn visit(pair: Pair, spare: Trace, take: bool, route: u8) -> Trace {
    let maybe;
    if take { maybe = Trace::Maybe; }
    match route {
        0 => { drop(pair.a); return Trace::Early; }
        1 => return spare,
        _ => {}
    }
    println!("continue");
    pair.b
}

fn main() {
    for route in [0, 1, 2] {
        for take in [false, true] {
            println!("route={route} take={take}");
            let pair = Pair { a: Trace::A, b: Trace::B };
            let result = visit(pair, Trace::Spare, take, route);
            println!("caller");
            drop(result);
        }
    }
}
