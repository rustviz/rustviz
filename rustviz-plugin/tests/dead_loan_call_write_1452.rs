// #1452: a call that borrows s8 for itself (`s8.push('!')`, `take(&mut
// s8)`, `s8 += ..`) on a path where r8's loan of s8 is dead though it
// lives on another path: the call's dot says why rustc accepts it, as for
// `let m = &mut s8;` there (#796). An assignment there (rb) gets no note.
fn take(s: &mut String) {
    s.push('?');
}

fn main() {
    let mut s8 = String::from("a");
    let n = 1;
    let r8 = &s8;
    if n > 1 {
        s8.push('!');
    } else {
        println!("{}", r8);
    }
    println!("{}", s8);

    let r9 = &s8;
    if n > 2 {
        take(&mut s8);
    } else {
        println!("{}", r9);
    }

    let ra = &s8;
    match n {
        0 => s8 += "!",
        _ => println!("{}", ra),
    }

    let rb = &s8;
    if n > 3 {
        s8 = String::from("b");
    } else {
        println!("{}", rb);
    }
    println!("{}", s8);
}
