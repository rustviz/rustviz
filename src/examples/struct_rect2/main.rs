/* --- BEGIN Variable Definitions ---
Struct r{width, height};
StaticRef rect;
Function area();
Function print_area();
--- END Variable Definitions --- */
struct Rectangle {
    width: u32,
    height: u32,
}

impl Rectangle {
    fn area(&self) -> u32 { // !{ InitializeParam(rect) }
        self.width * self.height
    } // !{ GoOutOfScope(rect) }
}

fn print_area(rect: &Rectangle) { // !{ InitializeParam(rect) }
    println!(
        "The area of the rectangle is {} square pixels.",
       	rect.area() // dot even though it's actually a reference !{ PassByStaticReference(rect->area()) }
    );
} // !{ GoOutOfScope(rect) }

fn main() {
    let r = Rectangle { // !{ Bind(None->r) }
        width: 30, // !{ Bind(None->width) }
        height: 50, // !{ Bind(None->height) }
    };
    
   	print_area(&r); // !{ PassByStaticReference(r->print_area()) }
} // !{ StructBox(r->height), GoOutOfScope(r), GoOutOfScope(width), GoOutOfScope(height) }