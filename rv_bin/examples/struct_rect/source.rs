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
    
    println!("The height of that is {}.", r.h);
}

fn area(rect: &Rect) -> u32 {
    rect.w * rect.h
}