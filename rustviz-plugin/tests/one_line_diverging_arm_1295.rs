// #1295: an arm of type `!` on one line (`0 => return n,`, `0 =>
// continue,`, `if c { return n; } else { … }`) has no rows of its own, so
// the columns it doesn't end are just the fork's diagonal into it. Like a
// longer diverging arm's column (#317), that diagonal ends in a stop bar;
// it used to stop in mid-air. A column with a dot of its own on that row
// ends at the dot instead (`0 => return s,` moves s; `return n;` copies n).

fn ret_if(c: bool) -> i32 {
    let s = String::from("s");
    let n = 5;
    if c { return n; } else { println!("{}", s); }
    n
}

fn cont_match(v: &Vec<i32>) {
    let s = String::from("s");
    for i in v {
        let k = *i;
        match k {
            0 => continue,
            _ => println!("{}", s),
        }
    }
}

fn panic_arm(n: i32) -> i32 {
    let s = String::from("s");
    let m = match n {
        0 => panic!("zero"),
        x => x,
    };
    println!("{}", s);
    m
}

fn return_value(n: i32) -> String {
    let s = String::from("a");
    let k = match n {
        0 => return s,
        m => m,
    };
    println!("{} {}", s, k);
    s
}

fn main() {
    ret_if(true);
    cont_match(&vec![1]);
    panic_arm(1);
    return_value(1);
}
