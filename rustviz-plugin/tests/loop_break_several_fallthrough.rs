// Several value `break`s (#561) carrying values declared inside the
// loop, when an iteration can also reach the end of the body: that
// iteration drops `k` and `j` there, and the one that ends the loop
// moves one of them into `r`.

fn main() {
    let mut n = 0;
    let r = loop {
        n += 1;
        let k = String::from("k");
        let j = String::from("j");
        if n == 3 {
            break k;
        }
        if n == 5 {
            break j;
        }
    };
    println!("{}", r);
}
