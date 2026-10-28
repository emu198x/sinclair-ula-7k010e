//! Sinclair 7K010E ULA — the 128K / +2 ZX Spectrum's custom chip.
//!
//! Source references:
//! - `knowledge/chips/sinclair-7k010e.md`
//! - `knowledge/systems/spectrum/contention.md`
//! - Adapted from `../Emu198x-Older/crates/sinclair-ula-7k010e/src/lib.rs`
//!
//! Same contention model as the Ferranti (48K) but different timing:
//! - Crystal: 17,734,475 Hz (4× PAL subcarrier)
//! - CPU divisor: 5 (not 4)
//! - 228 T-states/line (456 ULA clocks), 311 lines
//! - Contention phase 1 (pattern starts 1 T-state later in the line)
//! - Contention starts at T-state 14_361 (vs 14_335 on the 48K)

use emu198x_bus_sinclair_zx_spectrum::MemoryBus;
use emu198x_bus_sinclair_zx_spectrum::Ula;
use emu198x_bus_sinclair_zx_spectrum::timing::{self, FrameTiming};
use emu198x_sinclair_zx_spectrum_ula::{self as ula_engine, DELAY_TABLE_48K, UlaEngine};

/// Sinclair 7K010E ULA — the 128K / +2 ZX Spectrum's custom chip.
#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct SinclairUla {
    engine: UlaEngine,
}

impl SinclairUla {
    #[must_use]
    pub fn new() -> Self {
        Self {
            engine: UlaEngine::new(&ula_engine::CONFIG_128K),
        }
    }

    #[must_use]
    pub fn border_color(&self) -> u8 {
        self.engine.border
    }

    /// The speaker bit (bit 4) of the last write to port `$FE`.
    #[must_use]
    pub fn speaker(&self) -> bool {
        self.engine.beeper
    }

    /// Reinstall the 128K timing config after a snapshot restore.
    ///
    /// `UlaEngine::config` is `#[serde(skip)]` and falls back to the
    /// 48K config on deserialise (see `emu198x_sinclair_zx_spectrum_ula::
    /// default_config`). For the 128K class that's wrong:
    /// CPU divisor, line length, contention start, and the rest diverge.
    /// Call once after `restore`.
    pub fn reattach_config(&mut self) {
        self.engine.set_config(&ula_engine::CONFIG_128K);
    }
    /// Select the grey +2 interrupt phase at construction or state restore.
    /// All raster and contention settings remain in the same ULA family.
    pub fn reattach_plus2_config(&mut self) {
        self.engine.set_config(&ula_engine::CONFIG_PLUS2);
    }
}

impl Default for SinclairUla {
    fn default() -> Self {
        Self::new()
    }
}

impl Ula for SinclairUla {
    fn tick(
        &mut self,
        memory: &dyn MemoryBus,
        cpu_addr: u16,
        cpu_mreq: bool,
        cpu_iorq: bool,
        cpu_rfsh: bool,
        framebuffer: &mut [u8],
    ) {
        let e = &mut self.engine;
        // The 7K010E asserts /INT two T-states later than the 48K ULA.
        // Its physical display counter still opens the contention window
        // at C=0; the interrupt-relative 14361 coordinate is not C=-6.
        let phase = (e.scan < 192 && e.pixel < 256).then_some((usize::from(e.pixel) + 1) & 0x0f);

        // Snow: a CPU refresh with I in screen-RAM range collides with
        // the video fetch (the Sinclair ULA ignores /RFSH). gap #12.
        let snow = ula_engine::snow_address(cpu_rfsh, cpu_addr);

        e.tick_rendering(memory, framebuffer, snow);

        // FUSE spec128 uses the same early/late port rules as spec48,
        // with odd paged RAM banks contended. Score these independently
        // in machine-sinclair-zx-spectrum-128k/tests/io_contention_oracle.rs.
        if let Some(phase) = phase {
            let contended_addr = memory.is_contended(cpu_addr);
            let arming = e.gate_arms_this_halfcycle();
            let ula_port = cpu_addr & 1 == 0;
            // T1 falling: memory access or the first port lookup.
            let mem_contention = contended_addr && arming && !cpu_mreq && !cpu_iorq;
            // Even ports: one further lookup at T2 falling, regardless
            // of the page. Freeze the previous pin while the CPU stalls.
            let port_answered = ula_port && arming && cpu_iorq && !e.z80_iorq_prev;
            // Odd contended ports: three further falling-edge lookups.
            let port_unanswered = contended_addr && !ula_port && arming && cpu_iorq;

            let contention = mem_contention || port_answered || port_unanswered;
            e.cpu_clock = !(contention && DELAY_TABLE_48K[phase]);
        } else {
            e.cpu_clock = true;
        }

        e.track_z80_clock(cpu_iorq, cpu_mreq, cpu_iorq && (cpu_addr & 1) == 0);
    }

    fn cpu_clock_active(&self) -> bool {
        self.engine.cpu_clock
    }

    fn interrupt_active(&self) -> bool {
        self.engine.int_active
    }

    fn floating_bus(&self) -> u8 {
        if self.engine.idle {
            0xFF
        } else {
            self.engine.bus_data
        }
    }

    /// With no tape signal, bit 6 reads back the last speaker bit (bit 4)
    /// written, as on an Issue 3 48K.
    ///
    /// The 7K010E carries the 48K ULA's cassette output (Figure 20-2) and
    /// EAR input (Figure 20-3) on separate pins, 35 and 34, and the 128
    /// PCB ties them together, "reproducing the behaviour of the original
    /// ZX Spectrum 16/48K" (Smith, *The ZX Spectrum ULA*, ch. 24 p. 255).
    /// Smith judges the 7000-series array likely to share the 6000
    /// series' component values, so the bias at the EAR comparator is the
    /// 6C's: only the speaker bit lifts it past the threshold (ch. 20,
    /// Table 20-2). FUSE treats the 128K as Issue 3 the same way.
    fn read_fe(&self, port: u16, keyboard: &[u8; 8]) -> u8 {
        let ear = if self.engine.beeper { 0x40 } else { 0x00 };
        (self.engine.read_fe(port, keyboard) & !0x40) | ear
    }

    fn write_fe(&mut self, val: u8) {
        self.engine.write_fe(val);
    }

    fn frame_timing(&self) -> &FrameTiming {
        &timing::TIMING_128K
    }

    fn end_frame(&mut self) {
        self.engine.end_frame();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Committing an access must suppress contention that would
    /// otherwise fire — the `/MREQT23` term (Smith Chapter 18, pp.
    /// 192-193 and 197).
    ///
    /// Keying the gate off `MREQ` being inactive *right now* also matches
    /// the trailing T-state of a memory cycle, where the contended
    /// address is still on the bus, so every M-cycle past `M1` was
    /// charged a second full 8-T-state rotation. `M1` hid the fault: the
    /// cycle following its access is the refresh, whose address is
    /// uncontended, so single-M-cycle instructions measured exact while
    /// everything else drifted.
    ///
    /// Written as a differential rather than a fixed expectation. Two
    /// runs are driven from the same point with the same pin sequence
    /// except that one asserts `MREQ` to commit an access; the pixel
    /// counter advances whether or not the CPU clock is held, so the runs
    /// stay phase-aligned and are directly comparable. The test demands
    /// a tick where the committed run runs and the uncommitted run
    /// stalls. A gate without `MREQT23` produces identical traces —
    /// once `MREQ` is low again it has no memory of the access — so this
    /// cannot pass vacuously, and it does fail against the old gate.
    #[test]
    #[ignore = "KNOWN DIVERGENCE: passes only with the MREQT23 latch wired into the gate; \
                blocked on the floating-bus derivation — see \
                knowledge/decisions/spectrum-contention-vs-floating-bus.md"]
    fn a_committed_access_suppresses_contention_that_would_otherwise_fire() {
        fn clock_trace(commit: bool) -> Vec<bool> {
            let mut ula = SinclairUla::new();
            let mut fb = vec![0; timing::SCREEN_WIDTH * timing::SCREEN_HEIGHT];
            let tick = |ula: &mut _, mreq: bool, fb: &mut [u8]| {
                Ula::tick(ula, &ContendedMemory, 0x4000, mreq, false, false, fb);
            };

            // Into the contended window, then on to the free window that
            // releases the CPU — the delay table ends a stall, not MREQ.
            for _ in 0..256 {
                tick(&mut ula, false, &mut fb);
                if !ula.cpu_clock_active() {
                    break;
                }
            }
            for _ in 0..256 {
                tick(&mut ula, false, &mut fb);
                if ula.cpu_clock_active() {
                    break;
                }
            }

            // Optionally commit an access, then record what the gate does
            // once MREQ is low again in both runs.
            for _ in 0..3 {
                tick(&mut ula, commit, &mut fb);
            }
            (0..6)
                .map(|_| {
                    tick(&mut ula, false, &mut fb);
                    ula.cpu_clock_active()
                })
                .collect()
        }

        let committed = clock_trace(true);
        let uncommitted = clock_trace(false);

        assert!(
            uncommitted.iter().any(|running| !running),
            "the uncommitted run should stall, or the comparison proves nothing"
        );
        assert!(
            committed
                .iter()
                .zip(&uncommitted)
                .any(|(with, without)| *with && !*without),
            "committing an access did not suppress any contention: the \
             /MREQT23 term is missing, and the gate re-arms in the trailing \
             T-state, charging a second rotation to every M-cycle past M1\n\
             committed:   {committed:?}\n\
             uncommitted: {uncommitted:?}"
        );
    }

    struct ContendedMemory;

    impl MemoryBus for ContendedMemory {
        fn read(&self, _addr: u16) -> u8 {
            0
        }

        fn write(&mut self, _addr: u16, _value: u8) {}

        fn is_contended(&self, _addr: u16) -> bool {
            true
        }
    }

    #[test]
    fn frame_timing_matches_128k_constants() {
        let ula = SinclairUla::new();
        let t = ula.frame_timing();
        assert_eq!(t.master_hz, 17_734_475);
        assert_eq!(t.cpu_divisor, 5);
        assert_eq!(t.tstates_per_line, 228);
        assert_eq!(t.lines_per_frame, 311);
        assert_eq!(t.tstates_per_frame, 70_908);
        assert_eq!(t.contention_start_tstate, 14_361);
        assert_eq!(t.contention_phase, 1);
        assert_eq!(t.interrupt_length_tstates, 36);
    }

    #[test]
    fn defaults_to_white_border_and_no_interrupt() {
        let ula = SinclairUla::new();
        assert_eq!(ula.border_color(), 7);
        assert!(!ula.interrupt_active());
        assert_eq!(ula.floating_bus(), 0xFF);
    }

    #[test]
    fn write_fe_updates_border() {
        let mut ula = SinclairUla::new();
        ula.write_fe(0x05); // border bits 0..2 = 0b101 = magenta
        assert_eq!(ula.border_color(), 5);
    }

    #[test]
    fn contention_uses_the_counter_window_not_the_video_latch() {
        let mut ula = SinclairUla::new();
        let mut framebuffer = vec![0; timing::SCREEN_WIDTH * timing::SCREEN_HEIGHT];
        ula.engine.scan = 0;
        ula.engine.pixel = 3;
        ula.engine.z80_clock_high = false;
        ula.tick(
            &ContendedMemory,
            0x4000,
            false,
            false,
            false,
            &mut framebuffer,
        );
        assert!(
            !ula.cpu_clock_active(),
            "control phase 4 is busy before the first fetch"
        );
        assert!(!ula.engine.video, "fetches begin at counter 8");

        ula.engine.pixel = 259;
        ula.engine.video = true;
        ula.engine.z80_clock_high = false;
        ula.tick(
            &ContendedMemory,
            0x4000,
            false,
            false,
            false,
            &mut framebuffer,
        );
        assert!(ula.cpu_clock_active(), "contention ends at counter 256");
        assert!(ula.engine.video, "the final fetch pair is still in flight");

        ula.engine.scan = 310;
        ula.engine.pixel = 451;
        ula.engine.z80_clock_high = false;
        ula.tick(
            &ContendedMemory,
            0x4000,
            false,
            false,
            false,
            &mut framebuffer,
        );
        assert!(
            ula.cpu_clock_active(),
            "no contention on the preceding border line"
        );
    }

    /// Memory whose every byte is distinct enough to tell addresses apart:
    /// each read returns the high byte of the address XOR the low byte.
    struct PatternMemory {
        contended: bool,
    }

    impl PatternMemory {
        fn byte_at(addr: u16) -> u8 {
            let [high, low] = addr.to_be_bytes();
            high ^ low
        }
    }

    impl MemoryBus for PatternMemory {
        fn read(&self, addr: u16) -> u8 {
            Self::byte_at(addr)
        }

        fn write(&mut self, _addr: u16, _value: u8) {}

        fn is_contended(&self, _addr: u16) -> bool {
            self.contended
        }
    }

    const UNCONTENDED: PatternMemory = PatternMemory { contended: false };

    fn framebuffer() -> Vec<u8> {
        vec![0; timing::SCREEN_WIDTH * timing::SCREEN_HEIGHT]
    }

    /// One idle half-cycle: no strobes, an uncontended address.
    fn idle_tick(ula: &mut SinclairUla, fb: &mut [u8]) {
        ula.tick(&UNCONTENDED, 0x0000, false, false, false, fb);
    }

    /// Run scan line 248 from pixel 0 and return the pixel counts at which
    /// the interrupt rose and fell.
    fn interrupt_edges(ula: &mut SinclairUla) -> (u16, u16) {
        let mut fb = framebuffer();
        ula.engine.scan = 248;
        ula.engine.pixel = 0;
        let mut rose = None;
        let mut fell = None;
        while ula.engine.scan == 248 {
            let pixel = ula.engine.pixel;
            let before = ula.interrupt_active();
            idle_tick(ula, &mut fb);
            match (before, ula.interrupt_active()) {
                (false, true) => rose = Some(pixel),
                (true, false) => fell = Some(pixel),
                _ => {}
            }
        }
        (
            rose.expect("the interrupt should rise on line 248"),
            fell.expect("the interrupt should fall on line 248"),
        )
    }

    /// Put the ULA inside the contended window at a delay-table phase that
    /// withholds the clock, on a half-cycle where the gate can arm.
    fn ready_to_contend(ula: &mut SinclairUla) {
        ula.engine.scan = 0;
        ula.engine.pixel = 3;
        ula.engine.z80_clock_high = false;
        ula.engine.z80_iorq_prev = false;
    }

    #[test]
    fn default_is_the_same_as_new() {
        let ula = SinclairUla::default();
        assert_eq!(ula.border_color(), 7);
        assert!(!ula.speaker());
        assert!(ula.cpu_clock_active());
        assert_eq!((ula.engine.scan, ula.engine.pixel), (0, 0));
        assert_eq!(ula.frame_timing().tstates_per_frame, 70_908);
    }

    #[test]
    fn speaker_follows_bit_4_of_the_last_port_fe_write() {
        let mut ula = SinclairUla::new();
        assert!(!ula.speaker(), "the speaker starts low");

        ula.write_fe(0x10);
        assert!(ula.speaker());
        assert_eq!(ula.border_color(), 0, "bit 4 is not part of the border");

        ula.write_fe(0x0F);
        assert!(!ula.speaker(), "bits 0-3 leave the speaker low");
        assert_eq!(ula.border_color(), 7);
    }

    #[test]
    fn port_fe_bit_6_reads_back_the_speaker_bit() {
        let mut ula = SinclairUla::new();
        let no_keys = [0xFF; 8];

        assert_eq!(ula.read_fe(0xFEFE, &no_keys), 0xBF, "speaker low: EAR low");
        ula.write_fe(0x10);
        assert_eq!(
            ula.read_fe(0xFEFE, &no_keys),
            0xFF,
            "speaker high: EAR high"
        );
        ula.write_fe(0x08);
        assert_eq!(
            ula.read_fe(0xFEFE, &no_keys),
            0xBF,
            "the MIC bit alone does not lift EAR"
        );
    }

    #[test]
    fn port_fe_reads_only_the_half_rows_the_high_byte_selects() {
        let ula = SinclairUla::new();
        let mut keyboard = [0xFF; 8];
        keyboard[0] = 0xFE; // half-row 0, key 0 down (active low)

        assert_eq!(
            ula.read_fe(0xFEFE, &keyboard),
            0xBE,
            "A8 low selects half-row 0"
        );
        assert_eq!(
            ula.read_fe(0x7FFE, &keyboard),
            0xBF,
            "A15 low selects only half-row 7"
        );
        assert_eq!(
            ula.read_fe(0x00FE, &keyboard),
            0xBE,
            "every half-row selected"
        );
    }

    #[test]
    fn end_frame_rewinds_the_raster_and_drives_flash_every_16_frames() {
        let mut ula = SinclairUla::new();
        ula.engine.scan = 200;
        ula.engine.pixel = 100;
        ula.end_frame();
        assert_eq!((ula.engine.scan, ula.engine.pixel), (0, 0));

        for _ in 1..16 {
            ula.end_frame();
        }
        assert!(ula.engine.flash_active, "flash swaps after 16 frames");
        for _ in 0..16 {
            ula.end_frame();
        }
        assert!(!ula.engine.flash_active, "and swaps back after 16 more");
    }

    #[test]
    fn a_frame_of_half_cycles_is_456_pixels_by_311_lines() {
        let mut ula = SinclairUla::new();
        let mut fb = framebuffer();

        for _ in 0..456 {
            idle_tick(&mut ula, &mut fb);
        }
        assert_eq!((ula.engine.scan, ula.engine.pixel), (1, 0));

        // 70,908 T-states at two half-cycles each brings the raster home.
        for _ in 456..2 * 70_908 {
            idle_tick(&mut ula, &mut fb);
        }
        assert_eq!((ula.engine.scan, ula.engine.pixel), (0, 0));
    }

    #[test]
    fn interrupt_is_a_36_t_state_pulse_on_line_248() {
        let mut ula = SinclairUla::new();
        let (rose, fell) = interrupt_edges(&mut ula);
        assert_eq!((rose, fell), (5, 77));
        assert_eq!(
            (fell - rose) / 2,
            ula.frame_timing().interrupt_length_tstates as u16
        );
        assert!(!ula.interrupt_active());
    }

    #[test]
    fn the_plus2_config_moves_both_interrupt_edges_one_t_state_earlier() {
        let mut ula = SinclairUla::new();
        let toastrack = interrupt_edges(&mut ula);

        ula.reattach_plus2_config();
        let plus2 = interrupt_edges(&mut ula);
        assert_eq!(plus2, (toastrack.0 - 2, toastrack.1 - 2));

        // Raster geometry is unchanged: a line is still 456 pixels.
        let mut fb = framebuffer();
        ula.engine.scan = 10;
        ula.engine.pixel = 0;
        for _ in 0..456 {
            idle_tick(&mut ula, &mut fb);
        }
        assert_eq!((ula.engine.scan, ula.engine.pixel), (11, 0));
    }

    #[test]
    fn reattach_config_restores_the_128k_interrupt_after_the_plus2_config() {
        let mut ula = SinclairUla::new();
        ula.reattach_plus2_config();
        ula.reattach_config();
        assert_eq!(interrupt_edges(&mut ula), (5, 77));
    }

    #[test]
    fn floating_bus_shows_each_screen_fetch_then_idles_high() {
        let mut ula = SinclairUla::new();
        let mut fb = framebuffer();
        let seen: Vec<u8> = (0..16)
            .map(|_| {
                idle_tick(&mut ula, &mut fb);
                ula.floating_bus()
            })
            .collect();

        let bitmap0 = PatternMemory::byte_at(0x4000);
        let attr0 = PatternMemory::byte_at(0x5800);
        let bitmap1 = PatternMemory::byte_at(0x4001);
        let attr1 = PatternMemory::byte_at(0x5801);
        assert_eq!(
            seen,
            [
                0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, // before the fetches
                bitmap0, bitmap0, attr0, attr0, bitmap1, bitmap1, attr1, attr1,
            ]
        );

        // The bottom border fetches nothing.
        ula.engine.scan = 192;
        ula.engine.pixel = 8;
        idle_tick(&mut ula, &mut fb);
        assert_eq!(ula.floating_bus(), 0xFF);
    }

    #[test]
    fn a_refresh_in_screen_ram_replaces_the_bitmap_fetch_address() {
        fn fetch_at_pixel_8(ula: &mut SinclairUla, rfsh_addr: u16) -> u8 {
            let mut fb = framebuffer();
            ula.engine.scan = 0;
            ula.engine.pixel = 8;
            ula.tick(&UNCONTENDED, rfsh_addr, false, false, true, &mut fb);
            ula.floating_bus()
        }

        let mut ula = SinclairUla::new();
        assert_eq!(
            fetch_at_pixel_8(&mut ula, 0x4123),
            PatternMemory::byte_at(0x4123),
            "I in $40-$7F: snow"
        );
        assert_eq!(
            fetch_at_pixel_8(&mut ula, 0x0123),
            PatternMemory::byte_at(0x4000),
            "I outside screen RAM: a clean fetch"
        );
    }

    #[test]
    fn an_even_port_is_contended_even_at_an_uncontended_address() {
        let mut ula = SinclairUla::new();
        let mut fb = framebuffer();
        ready_to_contend(&mut ula);
        ula.tick(&UNCONTENDED, 0x00FE, false, true, false, &mut fb);
        assert!(!ula.cpu_clock_active(), "the ULA answers even ports");
    }

    #[test]
    fn an_even_port_lookup_is_not_repeated_while_iorq_stays_asserted() {
        let mut ula = SinclairUla::new();
        let mut fb = framebuffer();
        ready_to_contend(&mut ula);
        ula.engine.z80_iorq_prev = true;
        ula.tick(&UNCONTENDED, 0x00FE, false, true, false, &mut fb);
        assert!(ula.cpu_clock_active());
    }

    #[test]
    fn an_odd_port_is_contended_only_at_a_contended_address() {
        let mut fb = framebuffer();

        let mut ula = SinclairUla::new();
        ready_to_contend(&mut ula);
        ula.tick(&UNCONTENDED, 0x00FF, false, true, false, &mut fb);
        assert!(ula.cpu_clock_active(), "odd port, uncontended page");

        let mut ula = SinclairUla::new();
        ready_to_contend(&mut ula);
        ula.engine.z80_iorq_prev = true;
        ula.tick(
            &PatternMemory { contended: true },
            0x40FF,
            false,
            true,
            false,
            &mut fb,
        );
        assert!(
            !ula.cpu_clock_active(),
            "odd port, contended page: every lookup stalls"
        );
    }

    #[test]
    fn an_asserted_mreq_is_not_contended() {
        let mut ula = SinclairUla::new();
        let mut fb = framebuffer();
        ready_to_contend(&mut ula);
        ula.tick(&ContendedMemory, 0x4000, true, false, false, &mut fb);
        assert!(ula.cpu_clock_active());
    }

    #[test]
    fn port_contention_stops_at_the_bottom_border() {
        let mut ula = SinclairUla::new();
        let mut fb = framebuffer();
        ready_to_contend(&mut ula);
        ula.engine.scan = 192;
        ula.tick(&UNCONTENDED, 0x00FE, false, true, false, &mut fb);
        assert!(ula.cpu_clock_active());
    }

    #[test]
    fn a_contended_access_stalls_on_pixels_3_to_14_of_each_cycle() {
        let mut fb = framebuffer();
        let mut ula = SinclairUla::new();
        let stalled: Vec<u16> = (0..32)
            .filter(|&pixel| {
                ula.engine.scan = 0;
                ula.engine.pixel = pixel;
                ula.engine.z80_clock_high = false;
                ula.tick(&ContendedMemory, 0x4000, false, false, false, &mut fb);
                !ula.cpu_clock_active()
            })
            .collect();
        let expected: Vec<u16> = (3..15).chain(19..31).collect();
        assert_eq!(stalled, expected, "pixels 15, 0, 1 and 2 are free");
    }

    #[test]
    fn a_stall_holds_the_clock_phase_until_the_free_pixels() {
        let mut ula = SinclairUla::new();
        let mut fb = framebuffer();
        ready_to_contend(&mut ula);

        let mut stalls = 0;
        loop {
            ula.tick(&ContendedMemory, 0x4000, false, false, false, &mut fb);
            if ula.cpu_clock_active() {
                break;
            }
            stalls += 1;
            assert!(
                ula.engine.gate_arms_this_halfcycle(),
                "a stalled CPU's clock phase does not advance"
            );
        }
        assert_eq!(stalls, 12, "the stall runs from pixel 3 to pixel 14");
        assert_eq!(ula.engine.pixel, 16, "released by the tick at pixel 15");
    }

    #[test]
    fn a_border_write_reaches_the_framebuffer() {
        let mut ula = SinclairUla::new();
        let mut fb = framebuffer();
        ula.write_fe(0x02);
        while ula.engine.scan == 0 {
            idle_tick(&mut ula, &mut fb);
        }
        // Display line 0 is framebuffer row 48; ULA pixel 280 is in the
        // right border at framebuffer column 280 + 36.
        assert_eq!(fb[48 * timing::SCREEN_WIDTH + 316], 2);
    }
}
