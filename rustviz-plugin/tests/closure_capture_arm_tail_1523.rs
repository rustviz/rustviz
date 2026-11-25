// #1523: a closure in a value `if` / `match` arm borrows x, and the arm's
// value moves x. The borrow is the closure's, on the closure's line; the
// move is the value's, on its own line, after it. After the join x may
// have been moved (or was, when every arm moves it).
fn first(flag: bool, mut s: String) {
    let t = if flag {
        let mut c = || s.push('!');
        s
    } else {
        s
    };
}

fn second() {
    let flag = true;
    let s4 = String::from("ab");
    let s5 = if flag {
        let c2 = || s4.len();
        s4
    } else {
        String::from("hey")
    };
}

fn third(n: u8, s6: String) {
    let t3 = match n {
        0 => {
            let c3 = || s6.len();
            s6
        }
        _ => String::new(),
    };
}

fn fourth() {
    let flag = true;
    let mut s7 = String::from("ab");
    let t4 = if flag {
        let mut c4 = || s7.push('x');
        c4();
        s7
    } else {
        String::from("hey")
    };
}

fn main() {
    first(true, String::new());
    second();
    third(0, String::new());
    fourth();
}
