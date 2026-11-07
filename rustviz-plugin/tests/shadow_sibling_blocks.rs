// Two variables named `s` in sibling blocks are two variables, but
// neither shadows the other (the first is out of scope when the second
// is declared), so they share one column `s` (#173): the second one
// owns its own String and drops it at its block's end.

fn main() {
    {
        let s = String::from("a");
        drop(s);
    }
    {
        let s = String::from("b");
        println!("{}", s);
    }
}
