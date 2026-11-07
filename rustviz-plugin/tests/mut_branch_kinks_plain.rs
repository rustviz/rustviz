// Solid bends of a thick `let mut` column with no dot or icon on them (#487).

fn main() {
    let c = true;
    let mut n = 0;
    if c {
        println!("{}", c);
        n = 1;
        println!("{}", c);
    } else {
        println!("{}", c);
        n = 2;
        println!("{}", c);
    }
    println!("{}", n);
}
