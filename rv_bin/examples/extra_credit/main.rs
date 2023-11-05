/* --- BEGIN Variable Definitions ---
--- END Variable Definitions --- */
fn f(s1: &String) {
    s1.push_str(" 490!");
}

fn main() {
    let mut num = 490;
    let mut x = String::from("EECS");
    {
        let y = &mut x;
        f(y);
        let mut s2 = x;
        s2.push_str(" Woo!");
        println!("{}", s2);

        let n1 = num;
        println!("{}", n1);
    }
}