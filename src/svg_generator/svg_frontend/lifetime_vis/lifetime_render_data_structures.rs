use serde::Serialize;


#[derive(Debug, Clone, Serialize)]
pub struct FuncSignatureRenderHolder{
	pub x_val: u32,
	pub y_val: u32,
	pub segment: String,
	pub hover_msg: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct FuncSignatureStructInstanceHolder{
	pub x_val: u32,
	pub y_val: u32,
	pub segment: String,
	pub hover_msg: String,
}
#[derive(Debug, Clone, Serialize)]
pub struct LineNumberDashHolder{
	pub x1: u32,
	pub y1: u32,
	pub x2: u32,
	pub y2: u32,
	/**
	  * line number hovering above the dashed line
	  */
	pub line_number: u32,
}

#[derive(Debug, Clone, Serialize)]
pub struct VarLifetimeColumnHoler{
	pub data_hash: u32,
	pub x_anchor: u32,
	pub y_label: u32,
	/* lifetime start point y axis value */
	pub y_start: u32,
	/* lifetime end point y axis value */
	pub y_end: u32,
	/* determine the color and style of this column */
	pub label_name: String,
	pub NAME_msg: String,
	pub CRPT_msg: String,
	pub BODY_msg: String,
	pub DRPT_msg: String
}

impl VarLifetimeColumnHoler {
	pub fn new() -> VarLifetimeColumnHoler{
		VarLifetimeColumnHoler {
			x_anchor: 0,
			y_label: 0,
			y_start: 0,
			y_end: 0,
			data_hash: 0,
			label_name: "init".to_string(),
			NAME_msg: String::new(),
			CRPT_msg: String::new(),
			BODY_msg: String::new(),
			DRPT_msg: String::new(),
		}
	}
}

#[derive(Debug, Clone, Serialize)]

pub struct LifetimeParameterColumnSetHoler{
	/* name of lifetime parameter, with no ticks */
	pub lp_name: String,
	/* y position of lifetime label */
	pub y_label: u32,
	/* none negative number for styling */
	pub lifetime_hash: u32,
	/* x position of the dashed lifetime line */
	pub x_dash: u32,
	/* x position of the solid lifetime line  */
	/**
	 * `x_solid` = `x_dash` + 30
	 */
	pub x_solid: u32,
	/* y position of lp dashed line start point*/
	pub y_dash_start: u32,
	/* y position of lp dashed line end point*/
	pub y_dash_end: u32,
	/* y position of lifetime parameter line */
	/**
	 * `y_start` = `y_dash_start` + 20
	 */
	pub y_start: u32,
	/* y position of lifetime parameter line */
	/**
	 * `y_end` = `y_dash_end` - 20
	 */
	pub y_end: u32,
	/* x position of arrow head middle point */
	pub x_middle: u32,
	/* x position of arrow head left point */
	pub x_left: u32,
	/* x position of arrow head right point */
	pub x_right: u32,
	/* y position of upper arrow head first vertices */
	pub y_vertices_up: u32,
	/* y position of upper arrow head for the rest two vertices */
	pub y_line_up: u32,
	/* y position of bottom arrow head first vertices */
	pub y_vertices_bot: u32,
	/* y position of bottom arrow head for the rest two vertices */
	pub y_line_bot: u32,
	/* hover message for the dashed line */
	pub lp_dash_msg: String,
	/* hover message for upper arrow head */
	pub msg_up: String,
	/* hover message for solid lifetime parameter body */
	pub lp_solid_msg: String,
	/* hover message for bottom arrow head */
	pub msg_bot: String,
	/* hover message for lifetime label */
	pub lp_calc_text: String,
}


#[derive(Debug, Clone, Serialize)]
pub struct DoubleHeadedArrowHolder{
	/* data hash for styling in SVG */
	pub data_hash: u32,
	/* x position of the vertical arrow body as well as middle vertices */
	pub x_middle: u32,
	pub x_left: u32,
	pub x_right: u32,
	pub y_start: u32,
	pub y_end: u32,
	pub y_vertices_up: u32,
	pub y_vertices_bot: u32,
	pub msg: String
}
#[derive(Debug, Clone, Serialize)]

pub struct LifetimeRegionSquareHoler{
	pub lifetime_hash: u32,
	pub x: u32,
	pub y: u32,
	pub w: u32,
	pub h: u32,
	pub hover_msg: String,
}