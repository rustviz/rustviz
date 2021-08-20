fn main() {
    let x = 10;
    let y = 7;

    func(x, y);
}

fn func(i: u32, r: u32) {
    if i < 25 {
        println!(
            "Today, do {} pushups!",
            g(i)
        );
        println!(
            "Next, do {} situps!",
            g(i)
        );
    } else {
        if r == 3 {
            println!("Take a break today! Remember to stay hydrated!");
        } else {
            println!(
                "Today, run for {} minutes!",
                g(i)
            );
        }
    }
}