fn main() {
    let n = 7;
    let msg = match n {
        x if x < 0 => "negative",
        0 => "zero",
        _ => "positive",
    };
    println!("{}", msg);
}
