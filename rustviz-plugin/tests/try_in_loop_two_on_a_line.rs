// Two `?` on one line get one dot per owner, not two (#430); `?` in a
// loop drops what the function owns, not only what the loop declared.

fn num(s: &str) -> Result<i32, std::num::ParseIntError> {
    s.parse::<i32>()
}

fn sum(items: &[&str]) -> Result<i32, std::num::ParseIntError> {
    let label = String::from("sum");
    let mut total = 0;
    for pair in items.chunks(2) {
        let note = String::from("pair");
        total += num(pair[0])? + num(pair[1])?;
        println!("{} {}", note, total);
    }
    println!("{}", label);
    Ok(total)
}

fn main() {
    println!("{:?}", sum(&["1", "2"]));
}
