// #1049: the tooltip on a temporary dropped "at the end of this `&&` / `||`
// operand" quotes the operators, so its text has a bare `&` that must be
// escaped (`&amp;`) in the SVG attribute: the timeline has to be
// well-formed XML (an <object>-embedded or standalone SVG is rejected by
// the browser otherwise).
fn label() -> String {
    String::from("l")
}
fn main() {
    let n = 1;
    if label().len() > 0 && n > 0 {
        println!("then");
    }
    let b = n > 5 || label().is_empty();
    println!("{}", b);
}
