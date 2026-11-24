// #1410: a branch of a value `if` / `match` that uses a variable and then
// yields it. The use comes first in the branch and the move into the
// binding second (the binding used to be paired before the branch was
// visited, so its move came first and the use landed on a moved column,
// State::Invalid). In each context: `let`, `match`, a call argument's
// `Some(..)`, a struct field, an `else if` arm, a mutating use, a tuple.
struct W {
    s: String,
}

fn in_let() {
    let a = String::from("x");
    let c = std::env::args().count() > 1;
    let ta = if c {
        println!("{}", a);
        a
    } else {
        String::new()
    };
    println!("{}", ta);
}

fn in_match() {
    let b = String::from("x");
    let o = Some(1);
    let tb = match o {
        Some(_) => {
            println!("{}", b);
            b
        }
        None => String::new(),
    };
    println!("{}", tb);
}

fn in_ctor() {
    let d = String::from("x");
    let c = std::env::args().count() > 1;
    let td = Some(if c {
        println!("{}", d);
        d
    } else {
        String::new()
    });
    println!("{:?}", td);
}

fn in_field() {
    let e = String::from("x");
    let c = std::env::args().count() > 1;
    let w = W {
        s: if c {
            println!("{}", e);
            e
        } else {
            String::new()
        },
    };
    println!("{}", w.s);
}

fn in_else_if() {
    let f = String::from("x");
    let c = std::env::args().count() > 1;
    let tf = if c {
        String::new()
    } else if !c {
        println!("{}", f);
        f
    } else {
        String::new()
    };
    println!("{}", tf);
}

fn mutated() {
    let mut g = String::from("x");
    let c = std::env::args().count() > 1;
    let tg = if c {
        g.push('!');
        g
    } else {
        String::new()
    };
    println!("{}", tg);
}

fn in_tuple() {
    let h = String::from("x");
    let c = std::env::args().count() > 1;
    let (th, n) = if c {
        println!("{}", h);
        (h, 1)
    } else {
        (String::new(), 2)
    };
    println!("{} {}", th, n);
}

fn main() {
    in_let();
    in_match();
    in_ctor();
    in_field();
    in_else_if();
    mutated();
    in_tuple();
}
