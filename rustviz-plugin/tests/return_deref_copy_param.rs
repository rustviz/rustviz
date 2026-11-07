fn get(r: &i32) -> i32 {
    *r
}

fn bump(m: &mut i32) -> i32 {
    *m += 1;
    *m
}

fn main() {
    let mut x = 5;
    let a = get(&x);
    let b = bump(&mut x);
    println!("{} {}", a, b);
}
