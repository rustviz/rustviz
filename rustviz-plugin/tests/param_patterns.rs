struct P { name: String, age: u32 }

fn first((a, b): (String, i32)) -> usize {
    a.len() + b as usize
}

fn peek((s, n): &(String, i32)) -> usize {
    s.len() + *n as usize
}

fn add(&(x, y): &(i32, i32)) -> i32 {
    x + y
}

fn name_len(P { name, .. }: P) -> usize {
    name.len()
}

fn main() {
    let t = (String::from("a"), 1);
    let k = peek(&t);
    let n = first(t);
    let pair = (1, 2);
    let m = add(&pair);
    let p = P { name: String::from("b"), age: 3 };
    let l = name_len(p);
    println!("{} {} {} {}", k, n, m, l);
}
