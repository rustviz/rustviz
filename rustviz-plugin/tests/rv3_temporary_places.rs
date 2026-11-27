// Two instances of the same expression, one creation row, distinct storage.
fn make() -> String {
    String::from("made")
}

fn same_line(a: bool, b: bool) {
    let left = String::from("left");
    let right = String::from("right");
    let x = if a { &make() } else { &left }; let y = if b { &make() } else { &right };
    println!("{x} {y}");
}

#[derive(Debug)]
enum Part { First, Second, Inner, Outer, Only, Taken, Kept }
impl Drop for Part {
    fn drop(&mut self) {
        println!("{self:?}");
    }
}

fn pair() -> (Part, Part) {
    (Part::First, Part::Second)
}

fn items() -> (Part, Vec<Part>) {
    (Part::Outer, vec![Part::Inner])
}

fn shared(exit: bool) {
    for inner in items().1 {
        if exit { break; }
        drop(inner);
    }
}

// Both explicit returns reach one destructor. Fallthrough reaches another.
fn exit_only(a: bool, b: bool) {
    let (ref first, second) = pair();
    drop(second);
    if a { return; } if b { return; }
    println!("end {first:?}");
}

fn exceptional(fail: bool) {
    let (ref first, second) = pair();
    drop(second);
    if fail {
        panic!("unwind");
    }
    println!("normal {first:?}");
}

fn aborting() {
    let (ref first, second) = pair();
    drop(second);
    println!("held {first:?}");
    std::process::abort();
}

fn main() {
    same_line(true, false);
    shared(false);
    shared(true);
    exit_only(true, false);
    exit_only(false, true);
    exit_only(false, false);
    exceptional(false);
    let _ = std::panic::catch_unwind(|| exceptional(true));
    alternatives(false);
    alternatives(true);
}

enum Payload {
    One(Part),
    Two(Part, Part),
    Other(Part),
}

fn payload(two: bool) -> Payload {
    if two { Payload::Two(Part::Taken, Part::Kept) } else { Payload::One(Part::Only) }
}

fn alternatives(two: bool) {
    match payload(two) {
        Payload::One(x) | Payload::Two(x, _) => drop(x),
        Payload::Other(_) => {}
    }
}

fn bounds(index: usize) {
    let (ref first, second) = pair();
    drop(second);
    let value = [0_u8][index];
    println!("{first:?} {value}");
}
