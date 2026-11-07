// #281: the else branch reassigns `x`, which the condition may have
// moved. The old value is dropped there only if it's still in `x`
// (a runtime drop flag decides, #335).

fn main() {
    let mut x = Some(String::from("a"));
    if let Some(a) = x && a.len() > 5 {
        println!("{}", a);
    } else {
        x = Some(String::from("b"));
        println!("{:?}", x);
    }
}
