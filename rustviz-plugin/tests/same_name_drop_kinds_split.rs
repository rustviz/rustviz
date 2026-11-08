enum Light { Red, Green }

fn main() {
    {
        let x = String::from("a");
        println!("{}", x);
    }
    {
        let x = Light::Red;
        let y = x;
        let _ = y;
    }
}
