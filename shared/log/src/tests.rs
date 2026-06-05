use crate::*;

#[test]
fn sink_ring_caps_and_evicts_oldest() {
	let s = Sink::new();
	for i in 0..(MAX_ENTRIES + 10) {
		s.push(Entry {
			level: Level::Info,
			source: "t".into(),
			message: format!("{i}"),
			when_ms: i as u64,
		});
	}
	let snap = s.snapshot();
	assert_eq!(snap.len(), MAX_ENTRIES);
	assert_eq!(snap.first().unwrap().message, "10");
	assert_eq!(snap.last().unwrap().message, format!("{}", MAX_ENTRIES + 9));
}

#[test]
fn sink_clear_empties_ring() {
	let s = Sink::new();
	s.push(Entry {
		level: Level::Warn,
		source: "t".into(),
		message: "x".into(),
		when_ms: 0,
	});
	s.clear();
	assert!(s.snapshot().is_empty());
}
