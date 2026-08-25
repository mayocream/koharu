//! Canonical AMD targets recognized by `rocm-bootstrap`.
//!
//! Architecture names and KFD versions follow `rocm-systems` commit
//! `a022846cf553c2b135410a5168f97705f1b9c6ac`. Device types follow TheRock's
//! iGPU families, including legacy APUs retained by `rocm-bootstrap`.

use crate::DeviceType;
use DeviceType::{Gpu, IntegratedGpu};

pub(super) struct GfxTarget {
    pub(super) name: &'static str,
    #[allow(dead_code)] // Read by the Linux KFD probe; the table is shared with Windows.
    pub(super) version: i64,
    pub(super) device_type: DeviceType,
}

pub(super) const KNOWN_TARGETS: &[GfxTarget] = &[
    GfxTarget {
        name: "gfx900",
        version: 90_000,
        device_type: Gpu,
    },
    GfxTarget {
        name: "gfx902",
        version: 90_002,
        device_type: IntegratedGpu,
    },
    GfxTarget {
        name: "gfx904",
        version: 90_004,
        device_type: Gpu,
    },
    GfxTarget {
        name: "gfx906",
        version: 90_006,
        device_type: Gpu,
    },
    GfxTarget {
        name: "gfx908",
        version: 90_008,
        device_type: Gpu,
    },
    GfxTarget {
        name: "gfx909",
        version: 90_009,
        device_type: IntegratedGpu,
    },
    GfxTarget {
        name: "gfx90a",
        version: 90_010,
        device_type: Gpu,
    },
    GfxTarget {
        name: "gfx90c",
        version: 90_012,
        device_type: IntegratedGpu,
    },
    GfxTarget {
        name: "gfx942",
        version: 90_402,
        device_type: Gpu,
    },
    GfxTarget {
        name: "gfx950",
        version: 90_500,
        device_type: Gpu,
    },
    GfxTarget {
        name: "gfx1010",
        version: 100_100,
        device_type: Gpu,
    },
    GfxTarget {
        name: "gfx1011",
        version: 100_101,
        device_type: Gpu,
    },
    GfxTarget {
        name: "gfx1012",
        version: 100_102,
        device_type: Gpu,
    },
    GfxTarget {
        name: "gfx1013",
        version: 100_103,
        device_type: IntegratedGpu,
    },
    GfxTarget {
        name: "gfx1030",
        version: 100_300,
        device_type: Gpu,
    },
    GfxTarget {
        name: "gfx1031",
        version: 100_301,
        device_type: Gpu,
    },
    GfxTarget {
        name: "gfx1032",
        version: 100_302,
        device_type: Gpu,
    },
    GfxTarget {
        name: "gfx1033",
        version: 100_303,
        device_type: IntegratedGpu,
    },
    GfxTarget {
        name: "gfx1034",
        version: 100_304,
        device_type: Gpu,
    },
    GfxTarget {
        name: "gfx1035",
        version: 100_305,
        device_type: IntegratedGpu,
    },
    GfxTarget {
        name: "gfx1036",
        version: 100_306,
        device_type: IntegratedGpu,
    },
    GfxTarget {
        name: "gfx1100",
        version: 110_000,
        device_type: Gpu,
    },
    GfxTarget {
        name: "gfx1101",
        version: 110_001,
        device_type: Gpu,
    },
    GfxTarget {
        name: "gfx1102",
        version: 110_002,
        device_type: Gpu,
    },
    GfxTarget {
        name: "gfx1103",
        version: 110_003,
        device_type: IntegratedGpu,
    },
    GfxTarget {
        name: "gfx1150",
        version: 110_500,
        device_type: IntegratedGpu,
    },
    GfxTarget {
        name: "gfx1151",
        version: 110_501,
        device_type: IntegratedGpu,
    },
    GfxTarget {
        name: "gfx1152",
        version: 110_502,
        device_type: IntegratedGpu,
    },
    GfxTarget {
        name: "gfx1153",
        version: 110_503,
        device_type: IntegratedGpu,
    },
    GfxTarget {
        name: "gfx1200",
        version: 120_000,
        device_type: Gpu,
    },
    GfxTarget {
        name: "gfx1201",
        version: 120_001,
        device_type: Gpu,
    },
    GfxTarget {
        name: "gfx1250",
        version: 120_500,
        device_type: Gpu,
    },
    GfxTarget {
        name: "gfx1251",
        version: 120_501,
        device_type: Gpu,
    },
];
