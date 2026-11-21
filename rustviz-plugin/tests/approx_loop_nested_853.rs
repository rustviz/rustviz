// #853: at most one note per loop, also for nested loops (the `loop` below has
// no dot outside its `while let`, so only the `while let` gets a note),
// and for loops that declare no variable of their own (their
// note has no "the variables declared inside it are new on each pass").
fn main() {
    let mut total = 0;
    let grid = vec![vec![1, 2], vec![3]];
    while total < 100 {
        total += 10;
    }
    for row in grid.iter() {
        for cell in row.iter() {
            total += *cell;
        }
    }
    let mut stack = vec![String::from("a")];
    loop {
        while let Some(s) = stack.pop() {
            total += s.len();
        }
        break;
    }
    for _ in 0..2 {
        total += 1;
    }
    println!("{} {}", total, grid.len());
}
