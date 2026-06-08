//! Benchmarks for Timeline hot paths: indexed vs linear clip/track lookup,
//! batch insert throughput, and JSON serialization of large projects.

use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use rook_core::{
    AssetId, ClipId, TrackId, TrackKind,
    clip::Clip,
    time::Rational,
    timeline::Timeline,
    track::Track,
};

// ── Helpers ──────────────────────────────────────────────────────────────────

fn make_clip(asset_id: AssetId, pos: i64) -> Clip {
    Clip {
        id: ClipId::next(),
        label: "bench".into(),
        asset_id,
        timeline_in: pos,
        source_in: 0,
        source_duration: 100,
        transform: Default::default(),
        blend_mode: Default::default(),
        mask: None,
        fade: None,
        transition: None,
        speed: 1.0,
        speed_curve: vec![],
        reverse: false,
        freeze_frame: None,
        frame_blending: false,
        spatial_conform: None,
        gain_db: None,
        volume_keyframes: None,
        mute_audio: false,
        filters: vec![],
        keyframes: vec![],
        link_group_id: None,
        generator: None,
    }
}

/// Build a timeline with `n_tracks` tracks each holding `clips_per_track` clips.
/// Returns the timeline and a vec of all ClipIds (last clip of last track is target).
fn build_timeline(n_tracks: usize, clips_per_track: usize) -> (Timeline, Vec<ClipId>) {
    let mut tl = Timeline::new(Rational::FPS_24);
    let asset_id = AssetId::next();
    let mut all_clip_ids = Vec::with_capacity(n_tracks * clips_per_track);

    for i in 0..n_tracks {
        let mut track = Track::new(TrackKind::Video, format!("V{i}"), i);
        for j in 0..clips_per_track {
            let clip = make_clip(asset_id, (j * 120) as i64);
            all_clip_ids.push(clip.id);
            track.clips.push(clip);
        }
        tl.tracks.push(track);
    }
    (tl, all_clip_ids)
}

// ── Benchmarks ───────────────────────────────────────────────────────────────

/// Linear scan (index empty) vs rebuilt index lookup — same target: last clip.
fn bench_clip_lookup(c: &mut Criterion) {
    let sizes: &[(usize, usize)] = &[
        (4, 50),    // small: 200 clips
        (10, 100),  // medium: 1 000 clips
        (20, 200),  // large: 4 000 clips
    ];

    let mut group = c.benchmark_group("clip_lookup");
    for &(n_tracks, cpt) in sizes {
        let label = format!("{n_tracks}t×{cpt}c");

        // Without index (cleared)
        group.bench_with_input(
            BenchmarkId::new("linear", &label),
            &(n_tracks, cpt),
            |b, &(t, c)| {
                let (mut tl, ids) = build_timeline(t, c);
                let target = *ids.last().unwrap();
                tl.clip_track_index.clear();
                tl.track_pos_index.clear();
                b.iter(|| {
                    std::hint::black_box(tl.clip(target));
                });
            },
        );

        // With index rebuilt once
        group.bench_with_input(
            BenchmarkId::new("indexed", &label),
            &(n_tracks, cpt),
            |b, &(t, c)| {
                let (mut tl, ids) = build_timeline(t, c);
                let target = *ids.last().unwrap();
                tl.rebuild_index();
                b.iter(|| {
                    std::hint::black_box(tl.clip(target));
                });
            },
        );
    }
    group.finish();
}

fn bench_track_lookup(c: &mut Criterion) {
    let mut group = c.benchmark_group("track_lookup");
    for n_tracks in [10usize, 50, 200] {
        let (mut tl, _) = build_timeline(n_tracks, 1);
        let target = tl.tracks.last().unwrap().id;

        group.bench_with_input(
            BenchmarkId::new("linear", n_tracks),
            &n_tracks,
            |b, _| {
                tl.track_pos_index.clear();
                b.iter(|| std::hint::black_box(tl.track(target)));
            },
        );

        group.bench_with_input(
            BenchmarkId::new("indexed", n_tracks),
            &n_tracks,
            |b, _| {
                tl.rebuild_index();
                b.iter(|| std::hint::black_box(tl.track(target)));
            },
        );
    }
    group.finish();
}

/// Cost of rebuild_index itself.
fn bench_rebuild_index(c: &mut Criterion) {
    let mut group = c.benchmark_group("rebuild_index");
    for &(t, cpt) in &[(4, 50), (10, 100), (20, 200)] {
        let (mut tl, _) = build_timeline(t, cpt);
        group.bench_with_input(
            BenchmarkId::new("total_clips", t * cpt),
            &(t, cpt),
            |b, _| b.iter(|| tl.rebuild_index()),
        );
    }
    group.finish();
}

/// JSON serialization + deserialization round-trip.
fn bench_serde_roundtrip(c: &mut Criterion) {
    let mut group = c.benchmark_group("serde_roundtrip");
    for &(t, cpt) in &[(4, 50), (10, 100)] {
        let (tl, _) = build_timeline(t, cpt);
        let label = format!("{t}t×{cpt}c");

        group.bench_with_input(
            BenchmarkId::new("serialize", &label),
            &(t, cpt),
            |b, _| b.iter(|| std::hint::black_box(serde_json::to_string(&tl).unwrap())),
        );

        let json = serde_json::to_string(&tl).unwrap();
        group.bench_with_input(
            BenchmarkId::new("deserialize", &label),
            &json,
            |b, j| {
                b.iter(|| {
                    std::hint::black_box(serde_json::from_str::<Timeline>(j).unwrap())
                })
            },
        );
    }
    group.finish();
}

criterion_group!(
    benches,
    bench_clip_lookup,
    bench_track_lookup,
    bench_rebuild_index,
    bench_serde_roundtrip,
);
criterion_main!(benches);
