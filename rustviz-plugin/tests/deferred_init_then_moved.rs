// A `let` assigned on only some paths and moved on some of those
// (#244). s is assigned and moved in the if branch only: nothing is
// left to drop on any path. t is assigned in two arms and moved in one:
// its scope end drops its String only if the `1` arm ran.

fn main() {
    let c = true;
    let s;
    if c {
        s = String::from("x");
        drop(s);
    } else {
        println!("none");
    }

    let k = 1;
    let t;
    match k {
        0 => {
            t = String::from("zero");
            drop(t);
        }
        1 => {
            t = String::from("one");
            println!("{}", t);
        }
        _ => println!("none"),
    }
    println!("end");
}
