// `break` and `continue` leave the loop's iteration and drop what it
// declared (#410). `break 'outer` drops `c` and `b`; `continue` (of
// the inner loop) drops only `c`; `a` lives on after the loops.

fn show(s: &String) -> usize {
    s.len()
}

fn main() {
    let a = String::from("a");
    'outer: for i in 0..3 {
        let b = String::from("b");
        for j in 0..3 {
            let c = String::from("c");
            match i + j {
                2 => break 'outer,
                3 => continue,
                _ => {
                    show(&c);
                }
            }
        }
        show(&b);
    }
    show(&a);
}
