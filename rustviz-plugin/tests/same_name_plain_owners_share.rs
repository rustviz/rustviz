enum Light { Red, Green }
enum Mode { On, Off }

fn main() {
    {
        let x = Light::Red;
        let y = x;
        let _ = y;
    }
    {
        let x = Mode::On;
        let y = x;
        let _ = y;
    }
}
