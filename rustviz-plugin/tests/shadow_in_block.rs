// The Rust Book's shadowing examples (ch. 3.1, #173): a shadow that
// changes the type, and a shadow inside an inner block, which goes
// out of scope at the block's end while the outer `x` lives on.

fn main() {
    let spaces = "   ";
    let spaces = spaces.len();
    let x = 5;
    {
        let x = x * 2;
        println!("{}", x);
    }
    println!("{} {}", spaces, x);
}
