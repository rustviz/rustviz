struct Rect {
    w: u32,
    h: u32,
}

fn main() {
    let r = Rect {
        w: 30,
        h: 50,
    };

    println!(
        "The area of the rectangle is {} square pixels.",
        area(&r)
    );
}

fn area(rect: &Rect) -> u32 {
    rect.w * rect.h
}