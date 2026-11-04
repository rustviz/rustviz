pub static CODE_PANEL_TEMPLATE : &'static str = 
"<svg width=\"{{tl_width}}px\" height=\"{{height}}px\" xmlns=\"http://www.w3.org/2000/svg\" xmlns:xlink=\"http://www.w3.org/1999/xlink\">

    <desc>{{ visualization_name }}</desc>

    <defs>
        <style type=\"text/css\">
        <![CDATA[
        {{ css }}
        ]]>
        </style>
    </defs>

    <g>
        <text id=\"caption\" x=\"30\" y=\"30\">Hover over timeline events (dots), states (vertical lines),</text>
        <text id=\"caption\" x=\"30\" y=\"50\">and actions (arrows) for extra information.</text>
    </g>

    {{ code }}

</svg>";

pub static TIMELINE_PANEL_TEMPLATE: &'static str = 
"<svg width=\"{{tl_width}}px\" height=\"{{height}}px\" 
        xmlns=\"http://www.w3.org/2000/svg\" xmlns:xlink=\"http://www.w3.org/1999/xlink\" 
        id=\"{{tl_id}}\">

    <desc>{{ visualization_name }}</desc>

    <defs>
        <style type=\"text/css\">
        <![CDATA[
        {{ css }}
        
        text {
            user-select: none;
            -webkit-user-select: none;
            -moz-user-select: none;
            -ms-user-select: none;
        }
        ]]>
        </style>
        <!-- used when pass to function by ref -->
        <g id=\"functionDot\">
             <circle cx=\"0\" cy=\"0\" r=\"5\" fill=\"transparent\"/>
             <text class=\"functionIcon\" dx=\"-3.5\" dy=\"0\" fill=\"#6e6b5e\">f</text>
        </g>
        <marker id=\"arrowHead\" viewBox=\"0 0 10 10\"
            refX=\"0\" refY=\"4\"
            markerUnits=\"strokeWidth\"
            markerWidth=\"3px\" markerHeight=\"3px\"
            orient=\"auto\" fill=\"gray\">
            <path d=\"M 0 0 L 8.5 4 L 0 8 z\" fill=\"inherit\"/>
        </marker>
        <!-- glow highlight filter -->
        <filter id=\"glow\" x=\"-5000%\" y=\"-5000%\" width=\"10000%\" height=\"10000%\" filterUnits=\"userSpaceOnUse\">
            <feComposite in=\"flood\" result=\"mask\" in2=\"SourceGraphic\" operator=\"in\"></feComposite>
            <feGaussianBlur stdDeviation=\"2\" result=\"coloredBlur\"/>
            <feMerge>
                <feMergeNode in=\"coloredBlur\"></feMergeNode>
                <feMergeNode in=\"coloredBlur\"></feMergeNode>
                <feMergeNode in=\"coloredBlur\"></feMergeNode>
                <feMergeNode in=\"SourceGraphic\"></feMergeNode>
            </feMerge>
            <!-- increase brightness -->
            <feComponentTransfer>
                <feFuncR type=\"linear\" slope=\"2\"/>
                <feFuncG type=\"linear\" slope=\"2\"/>
                <feFuncB type=\"linear\" slope=\"2\"/>
            </feComponentTransfer>
        </filter>
    </defs>

    {{ diagram }}

</svg>";


pub static CSS_TEMPLATE: &'static str = 
"/* general setup */
    :root {
        --bg-color:#f1f1f1;
        --text-color: #6e6b5e;
    }
    
    svg {
        background-color: var(--bg-color);
    }
    
    text {
        vertical-align: baseline;
        text-anchor: start;
    }
    
    #heading {
        font-size: 24px;
        font-weight: bold;
    }
    
    #caption {
        font-size: 0.875em;
        font-family: \"Open Sans\", sans-serif;
        font-style: italic;
    }
    
    /* code related styling */
    text.code {
        fill: #6e6b5e;
        white-space: pre;
        font-family: \"Source Code Pro\", Consolas, \"Ubuntu Mono\", Menlo, \"DejaVu Sans Mono\", monospace, monospace !important;
        font-size: 0.875em;
    }

    text.code .kw {
        font-weight: bold;
    }

    text.code .comment {
        fill: #8a8a8a;
        font-style: italic;
    }

    text.code .string {
        fill: #3a6bb5;
    }

    text.label {
        font-family: \"Source Code Pro\", Consolas, \"Ubuntu Mono\", Menlo, \"DejaVu Sans Mono\", monospace, monospace !important;
        font-size: 0.875em;
    }
    
    /* timeline/event interaction styling */
    .solid {
        stroke-width: 5px;
    }
    
    .hollow {
        stroke-width: 1.5;
    }
    
    .dotted {
        stroke-width: 5px;
        stroke-dasharray: \"2 1\";
    }
    
    .extend {
        stroke-width: 1px;
        stroke-dasharray: \"2 1\";
    }
    
    .functionIcon {
        paint-order: stroke;
        stroke-width: 3px;
        fill: var(--bg-color);
        font-size: 20px;
        font-family: times;
        font-weight: lighter;
        dominant-baseline: central;
        text-anchor: start;
        font-style: italic;
    }
    
    .functionLogo {
        font-size: 20px;
        font-style: italic;
        paint-order: stroke;
        stroke-width: 3px;
        fill: var(--bg-color) !important;
    }
    
    /* flex related styling */
    .flex-container {
        display: flex;
        flex-direction: row;
        justify-content: flex-start;
        flex-wrap: nowrap;
        flex-shrink: 0;
    }
    
    object.tl_panel {
        flex-grow: 1;
    }
    
    object.code_panel {
        flex-grow: 0;
    }
    
    .tooltip-trigger {
        cursor: default;
    }
    
    .tooltip-trigger:hover{
        filter: url(#glow);
    }

    /* When the cursor is anywhere inside a branch column,
       glow every element grouped under .branch-group.
       Single-element hover otherwise only highlighted one of
       the two parallel sides at a time, with no feedback on
       the transparent gap between them. */
    .branch-group:hover .tooltip-trigger {
        filter: url(#glow);
    }
";



/// Column colours, indexed by `data-hash`. Hash 0 is the code panel's
/// colour for function names (fill only); every resource access point
/// and called function gets the next hash from 1 up.
const HASH_PALETTE: [&str; 13] = [
    "#6e6b5e", // 0: function names
    "#1893ff", "#ff7f50", "#8635ff", "#dc143c", "#0a810a", "#008080",
    "#ff6cce", "#00d6fc", "#b99f35", "#15a710", "#000000", "#463ae9",
];

/// The `[data-hash="N"]` colour rules for hashes `0..=max_hash` (at
/// least the whole palette). Hashes past the palette reuse its colours
/// 1..=12 in order, so every column's state lines get a stroke however
/// many variables the program has. Without a rule a column's lines had
/// no stroke at all (#279). Only the colour cycles: `data-hash` itself
/// stays unique, since it's also what links code-panel names to their
/// columns on hover.
pub fn hash_css(max_hash: u64) -> String {
    let cycle = (HASH_PALETTE.len() - 1) as u64;
    let last = max_hash.max(cycle);
    let mut css = String::from("\n    /* hash based styling */\n");
    css.push_str(&format!(
        "    [data-hash=\"0\"] {{ fill: {}; }}\n",
        HASH_PALETTE[0]
    ));
    for h in 1..=last {
        let c = HASH_PALETTE[((h - 1) % cycle + 1) as usize];
        css.push_str(&format!(
            "    [data-hash=\"{h}\"] {{ fill: {c}; stroke: {c}; }}\n"
        ));
    }
    css
}

/// Largest hash an SVG fragment refers to, as `data-hash="N"` or as a
/// function name's `hash="N"` (which the hover script copies into
/// `data-hash` to colour the name).
pub fn max_hash_in(svg: &str) -> u64 {
    svg.split("hash=\"")
        .skip(1)
        .filter_map(|rest| rest.split('"').next()?.parse::<u64>().ok())
        .max()
        .unwrap_or(0)
}
