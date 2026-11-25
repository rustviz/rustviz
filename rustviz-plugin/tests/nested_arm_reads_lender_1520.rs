// #1520: a &mut used in both outer arms; an inner conditional reads the lender on one path.
fn f_1520a() {
    let mut a = String::from("x");
    let m = &mut a;
    let n = 3;
    if n > 1 {
        if n > 2 {
            println!("{}", a);
        } else {
            println!("{}", m);
        }
    } else {
        println!("{}", m);
    }
}
fn f_s1520_match() {
    let mut a = String::from("x");
    let m = &mut a;
    let n = 3;
    if n > 1 {
        match n {
            2 => println!("{}", a),
            _ => println!("{}", m),
        }
    } else {
        drop(m);
    }
}
fn f_s1520_three() {
    let mut a = String::from("x");
    let m = &mut a;
    let n = 3;
    if n > 1 {
        if n > 2 {
            if n > 3 {
                a.push('a');
            } else {
                m.push('b');
            }
        } else {
            m.push('c');
        }
    } else {
        m.push('d');
    }
}
fn main() {}
