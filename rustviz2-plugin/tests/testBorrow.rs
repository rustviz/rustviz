fn square(x: &i32) -> i32 {
    x * x
}

fn main() {
    let num = 4;
    let result = square(&num);
    println!("The square of {} is {}", num, result);
}