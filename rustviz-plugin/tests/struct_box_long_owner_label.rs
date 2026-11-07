struct Pair { a: i32, b: i32 }

fn main() {
    let k = 5;
    let measurement_pair = Pair { a: k, b: 2 };
    let n = measurement_pair.a + measurement_pair.b;
    println!("{}", n);
}
