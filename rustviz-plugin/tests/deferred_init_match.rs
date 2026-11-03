// Deferred initialization in every arm of a match (#234).

fn main() {
    let n = 1;
    let s;
    match n {
        0 => {
            s = String::from("zero");
        }
        1 => {
            s = String::from("one");
        }
        _ => {
            s = String::from("many");
        }
    }
    println!("{}", s);
}
