// #1390: a destructuring `for` head draws one arrow per binding from the
// collection it takes each element apart from, as `let (s, k) = pair;`
// draws one from each field: a move of the part that needs drop, a copy
// of a Copy part. The collection's dot says it is moved into the loop's
// iterator (or, for `&pairs`, borrowed by it). No arrow left it before.
struct P { name: String, n: i32 }

fn main() {
    let names = vec![(String::from("n"), 1)];
    for (s, k) in names {
        println!("{} {}", s, k);
    }
    let pairs = vec![(1, 2)];
    for (p, q) in pairs {
        println!("{} {}", p, q);
    }
    let ps = vec![P { name: String::from("x"), n: 3 }];
    for P { name, n } in ps {
        println!("{} {}", name, n);
    }
    let nest = vec![((String::from("a"), 2), String::from("b"))];
    for ((u, w), z) in nest {
        println!("{} {} {}", u, w, z);
    }
    let firsts = vec![(String::from("f"), String::from("g"))];
    for (f, _) in firsts {
        println!("{}", f);
    }
    let kept = vec![(3, 4)];
    for &(x, y) in &kept {
        println!("{} {}", x, y);
    }
    println!("{}", kept.len());
}
