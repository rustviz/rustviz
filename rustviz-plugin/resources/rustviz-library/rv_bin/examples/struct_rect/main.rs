/* --- BEGIN Variable Definitions ---
Struct r{w, h}; StaticRef rect;
Function area(); Function println!();
--- END Variable Definitions --- */
struct Rect {
    w: u32,
    h: u32,
}

fn main() {
    let r = Rect { // !{ Bind(r) }
        w: 30, // !{ Bind(r.w) }
        h: 50 // !{ Bind(r.h) }
    };

    println!(
        "The area of the rectangle is {} square pixels.",
        area(&r) // !{ PassByStaticReference(r->area()) }
    );

    println!("The height of that is {}.", r.h); // !{ PassByStaticReference(r.h->println!()) }
} // !{ GoOutOfScope(r.w), GoOutOfScope(r.h), GoOutOfScope(r) }

fn area(rect: &Rect) -> u32 { // !{ InitRefParam(rect) }
    rect.w * rect.h
} // !{ GoOutOfScope(rect) }