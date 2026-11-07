// Same-named variables of sibling blocks share one column (#308); each
// one ends at its own block's `}` (#589).

// Two sibling blocks, each owning a String named `t`: both are dropped,
// one at each closing brace.
fn sibling_strings() {
    {
        let t = String::from("a");
        println!("{}", t);
    }
    {
        let t = String::from("b");
        println!("{}", t);
    }
}

// Three siblings, and a reference declared in each.
fn three_siblings() {
    let n = 1;
    {
        let r = &n;
        println!("{}", r);
    }
    {
        let r = &n;
        println!("{}", r);
    }
    {
        let r = &n;
        println!("{}", r);
    }
}

fn main() {
    sibling_strings();
    three_siblings();
}
