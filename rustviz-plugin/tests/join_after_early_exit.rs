// The merge after a conditional whose branches that move a value all
// leave early (#350): on every path that reaches the merge the value
// is still owned, and the merge says how the moving branches left.
fn take(_s: String) {}

fn panics(n: i32) {
    let s = String::from("a");
    if n < 0 {
        take(s);
        panic!("negative");
    }
    println!("{}", s);
}

fn continues(v: &[i32]) {
    for x in v {
        let c = String::from("b");
        if *x < 0 {
            take(c);
            continue;
        }
        println!("{}", c);
    }
}

fn two_returns(n: i32) -> String {
    let t = String::from("c");
    match n {
        0 => return t,
        1 => {
            take(t);
            return String::new();
        }
        _ => {}
    }
    t
}

fn mixed(v: &[i32]) -> i32 {
    let mut total = 0;
    for x in v {
        let m = String::from("d");
        match *x {
            0 => {
                take(m);
                break;
            }
            1 => {
                take(m);
                return total;
            }
            _ => total += x,
        }
        println!("{}", m);
    }
    total
}

fn ref_kept(r: &mut String, n: i32) -> &mut String {
    if n > 0 {
        return r;
    }
    r.push('x');
    r
}

fn main() {
    panics(1);
    continues(&[1, -1]);
    println!("{}", two_returns(2));
    println!("{}", mixed(&[2, 1]));
    let mut s = String::new();
    ref_kept(&mut s, 0);
}
