struct Rectangle {
    <tspan data-hash="2">width</tspan>: u32,
    <tspan data-hash="3">height</tspan>: u32,
}

impl Rectangle {
    fn area(&amp;self) -&gt; u32 {
        self.width * self.height
    }
}

fn <tspan class="fn" data-hash="0" hash="7">print_area</tspan>(<tspan data-hash="4">rect</tspan>: &amp;Rectangle) {
    println!(
        "The area of the rectangle is {} square pixels.",
       	<tspan data-hash="4">rect</tspan>.<tspan class="fn" data-hash="0" hash="6">area</tspan>() // dot even though it's actually a reference
    );
}

fn main() {
    let <tspan data-hash="1">r</tspan> = Rectangle {
        <tspan data-hash="2">width</tspan>: 30,
        <tspan data-hash="3">height</tspan>: 50,
    };

    <tspan class="fn" data-hash="0" hash="7">print_area</tspan>(<tspan data-hash="1">&amp;r</tspan>);
}
