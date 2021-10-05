struct Rect {
    w: u32,
    h: u32,
}

fn main() {
    let <tspan data-hash="1">r</tspan> = Rect {
        w: 30,
        h: 50,
    };

    <tspan class="fn" data-hash="0" hash="6">println!</tspan>(
        "The area of the rectangle is {} square pixels.",
        <tspan class="fn" data-hash="0" hash="5">area</tspan>(&r)
    );
    
    <tspan class="fn" data-hash="0" hash="6">println!</tspan>("The height of that is {}.", r.h);
}

fn <tspan class="fn" data-hash="0" hash="5">area</tspan>(<tspan data-hash="4">rect: &Rect</tspan>) -> u32 {
    rect.w * rect.h
}
