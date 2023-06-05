/* --- BEGIN Variable Definitions ---
Owner y;
Function println!();
Owner x;
Function String::from();
--- END Variable Definitions --- */
fn main() {
    let x = String::from("hello");
    let y = x;
    println!("{}", y);
}