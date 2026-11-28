enum Trace {
    Token(&'static str),
}
impl Trace {
    fn label(&self) -> &'static str {
        match self {
            Trace::Token(label) => label,
        }
    }
}
impl Drop for Trace {
    fn drop(&mut self) { println!("drop {}", self.label()); }
}
#[inline(never)]
fn made(name: &'static str) -> Trace {
    println!("rhs {name}");
    Trace::Token(name)
}

fn order() {
    let mut value = Trace::Token("old");
    value =
        made("new");
    println!("stored {}", value.label());
}

fn destructure() {
    let (mut left, mut right, mut last) = (Trace::Token("left"), Trace::Token("right"), Trace::Token("last"));
    let incoming = made("replacement");
    (left, right, last) = (right, left, incoming);
    println!("stored {} {} {}", left.label(), right.label(), last.label());
}

fn early(exit: bool) {
    let mut value = Trace::Token("before exit");
    value = if exit {
        println!("leaving RHS");
        return;
    } else {
        made("after exit test")
    };
    println!("stored {}", value.label());
}

fn partial(flag: bool) {
    let mut boxed = Box::new((Trace::Token("box part 0"), Trace::Token("box part 1")));
    let mut array = [Trace::Token("array part 0"), Trace::Token("array part 1")];
    if flag {
        drop(boxed.0);
        let [first, _] = array;
        drop(first);
    }
    *boxed = (made("box 0"), made("box 1"));
    array = [made("array 0"), made("array 1")];
    println!("stored {} {}", boxed.0.label(), array[0].label());
}

struct Pair { left: Trace, right: Trace }
fn fields() {
    let mut pair = Pair { left: Trace::Token("old left"), right: Trace::Token("old right") };
    let one = made("new right");
    let two = made("new left");
    let three = made("new right again");
    let copied = 7;
    let mut n = 0;
    pair.right = one; n = copied; pair.left = two; pair.right = three;
    println!("stored {} {}", pair.left.label(), pair.right.label());
    println!("{n}");
}

fn index(ticket: Trace) -> usize {
    println!("evaluating the index using {}", ticket.label());
    0
}

fn leaves_before_place() {
    let mut values = [Trace::Token("old array")];
    let ticket = Trace::Token("index ticket");
    values[index(ticket)] = return;
}

fn main() {
    order();
    destructure();
    early(true);
    early(false);
    partial(true);
    partial(false);
    fields();
    leaves_before_place();
}
