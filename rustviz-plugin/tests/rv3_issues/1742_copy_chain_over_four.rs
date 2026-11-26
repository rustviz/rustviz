fn main() {
    let mut s = String::from("a");
    let c = std::env::args().count() > 1;
    let r0: &String = &mut s;
    let q0 = r0;
    if c {
        let q1 = q0;
        let q2 = q1;
        let q3 = q2;
        let q4 = q3;
        let q5 = q4;
        let q6 = q5;
        let q7 = q6;
        println!("{}", q7);
    }
    println!("{}", q0);
}
