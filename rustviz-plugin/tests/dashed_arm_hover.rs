// #626: an arm's column is dashed on the rows where another arm's code
// runs and solid from its own arm onward. Hovering a dashed stretch
// says so; the solid stretches keep the ownership text.
fn take(_s: String) {}

fn main() {
    let s = String::from("hi");
    let c = true;
    if c {
        println!("a");
        take(s);
    } else {
        println!("b");
        println!("{}", s);
    }
    let t = String::from("there");
    let n = 3;
    match n {
        0 => {
            println!("zero");
            take(t);
        }
        _ => {
            println!("many");
            println!("{}", t);
        }
    }
    let opt: Option<String> = Some(String::from("x"));
    if let Some(x) = opt {
        println!("{}", x);
    }
}
