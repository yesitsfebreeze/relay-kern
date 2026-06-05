mod sel_cursor {
	use crate::list_nav::*;

	#[test]
	fn up_clamps_at_zero() {
		let mut c = SelCursor::new(3);
		c.move_up();
		assert_eq!(c.sel(), 0);
	}

	#[test]
	fn down_clamps_at_end() {
		let mut c = SelCursor::new(3);
		for _ in 0..10 {
			c.move_down();
		}
		assert_eq!(c.sel(), 2);
	}

	#[test]
	fn empty_ignores_moves() {
		let mut c = SelCursor::new(0);
		c.move_down();
		c.move_up();
		assert_eq!(c.sel(), 0);
	}
}
