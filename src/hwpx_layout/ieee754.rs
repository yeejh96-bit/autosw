//! Exact operation-order port of the Node 24.22.1 / V8 13.6 fdlibm sin/cos.
//! Source: https://github.com/nodejs/node/blob/v24.22.1/deps/v8/src/base/ieee754.cc
//! Only the double-precision trigonometric path is retained. In particular,
//! kernel_rem_pio2 uses precision 2, the sole mode used by this source's callers.
//
// Copyright (C) 1993 by Sun Microsystems, Inc. All rights reserved.
// Developed at SunSoft, a Sun Microsystems, Inc. business.
// Permission to use, copy, modify, and distribute this software is freely
// granted, provided that this notice is preserved.
// The original source has been modified significantly by Google Inc.
// Copyright 2016 the V8 project authors. All rights reserved.
// V8 additional license: https://github.com/nodejs/node/blob/v24.22.1/deps/v8/LICENSE
//
// Copyright 2014, the V8 project authors. All rights reserved.
// Redistribution and use in source and binary forms, with or without
// modification, are permitted provided that the following conditions are met:
// * Redistributions of source code must retain the above copyright notice,
//   this list of conditions and the following disclaimer.
// * Redistributions in binary form must reproduce the above copyright notice,
//   this list of conditions and the following disclaimer in the documentation
//   and/or other materials provided with the distribution.
// * Neither the name of Google Inc. nor the names of its contributors may be
//   used to endorse or promote products derived from this software without
//   specific prior written permission.
// THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS IS"
// AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE
// IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE
// ARE DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT OWNER OR CONTRIBUTORS BE
// LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR
// CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF
// SUBSTITUTE GOODS OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS
// INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN
// CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE)
// ARISING IN ANY WAY OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE
// POSSIBILITY OF SUCH DAMAGE.

// Indexed loops intentionally retain the original summation and reduction order.
#![allow(clippy::needless_range_loop)]

const TWO24: f64 = 16777216.0;
const TWON24: f64 = f64::from_bits(0x3e70000000000000);
const INVPIO2: f64 = f64::from_bits(0x3fe45f306dc9c883);
const PIO2_1: f64 = f64::from_bits(0x3ff921fb54400000);
const PIO2_1T: f64 = f64::from_bits(0x3dd0b4611a626331);
const PIO2_2: f64 = f64::from_bits(0x3dd0b4611a600000);
const PIO2_2T: f64 = f64::from_bits(0x3ba3198a2e037073);
const PIO2_3: f64 = f64::from_bits(0x3ba3198a2e000000);
const PIO2_3T: f64 = f64::from_bits(0x397b839a252049c1);
const TWO_OVER_PI: [i32; 66] = [
    0xA2F983, 0x6E4E44, 0x1529FC, 0x2757D1, 0xF534DD, 0xC0DB62, 0x95993C, 0x439041, 0xFE5163,
    0xABDEBB, 0xC561B7, 0x246E3A, 0x424DD2, 0xE00649, 0x2EEA09, 0xD1921C, 0xFE1DEB, 0x1CB129,
    0xA73EE8, 0x8235F5, 0x2EBB44, 0x84E99C, 0x7026B4, 0x5F7E41, 0x3991D6, 0x398353, 0x39F49C,
    0x845F8B, 0xBDF928, 0x3B1FF8, 0x97FFDE, 0x05980F, 0xEF2F11, 0x8B5A0A, 0x6D1F6D, 0x367ECF,
    0x27CB09, 0xB74F46, 0x3F669E, 0x5FEA2D, 0x7527BA, 0xC7EBE5, 0xF17B3D, 0x0739F7, 0x8A5292,
    0xEA6BFB, 0x5FB11F, 0x8D5D08, 0x560330, 0x46FC7B, 0x6BABF0, 0xCFBC20, 0x9AF436, 0x1DA9E3,
    0x91615E, 0xE61B08, 0x659985, 0x5F14A0, 0x68408D, 0xFFD880, 0x4D7327, 0x310606, 0x1556CA,
    0x73A8C9, 0x60E27B, 0xC08C6B,
];
const NPIO2_HW: [u32; 32] = [
    0x3FF921FB, 0x400921FB, 0x4012D97C, 0x401921FB, 0x401F6A7A, 0x4022D97C, 0x4025FDBB, 0x402921FB,
    0x402C463A, 0x402F6A7A, 0x4031475C, 0x4032D97C, 0x40346B9C, 0x4035FDBB, 0x40378FDB, 0x403921FB,
    0x403AB41B, 0x403C463A, 0x403DD85A, 0x403F6A7A, 0x40407E4C, 0x4041475C, 0x4042106C, 0x4042D97C,
    0x4043A28C, 0x40446B9C, 0x404534AC, 0x4045FDBB, 0x4046C6CB, 0x40478FDB, 0x404858EB, 0x404921FB,
];
fn high(x: f64) -> u32 {
    (x.to_bits() >> 32) as u32
}
fn kernel_cos(x: f64, y: f64) -> f64 {
    const C1: f64 = f64::from_bits(0x3fa555555555554c);
    const C2: f64 = f64::from_bits(0xbf56c16c16c15177);
    const C3: f64 = f64::from_bits(0x3efa01a019cb1590);
    const C4: f64 = f64::from_bits(0xbe927e4f809c52ad);
    const C5: f64 = f64::from_bits(0x3e21ee9ebdb4b1c4);
    const C6: f64 = f64::from_bits(0xbda8fae9be8838d4);
    let ix = high(x) & 0x7fffffff;
    if ix < 0x3e400000 {
        return 1.0;
    }
    let z = x * x;
    let r = z * (C1 + z * (C2 + z * (C3 + z * (C4 + z * (C5 + z * C6)))));
    if ix < 0x3fd33333 {
        1.0 - (0.5 * z - (z * r - x * y))
    } else {
        let qx = if ix > 0x3fe90000 {
            0.28125
        } else {
            f64::from_bits(u64::from(ix - 0x00200000) << 32)
        };
        let iz = 0.5 * z - qx;
        let a = 1.0 - qx;
        a - (iz - (z * r - x * y))
    }
}
fn kernel_sin(x: f64, y: f64, tail: bool) -> f64 {
    const S1: f64 = f64::from_bits(0xbfc5555555555549);
    const S2: f64 = f64::from_bits(0x3f8111111110f8a6);
    const S3: f64 = f64::from_bits(0xbf2a01a019c161d5);
    const S4: f64 = f64::from_bits(0x3ec71de357b1fe7d);
    const S5: f64 = f64::from_bits(0xbe5ae5e68a2b9ceb);
    const S6: f64 = f64::from_bits(0x3de5d93a5acfd57c);
    if high(x) & 0x7fffffff < 0x3e400000 {
        return x;
    }
    let z = x * x;
    let v = z * x;
    let r = S2 + z * (S3 + z * (S4 + z * (S5 + z * S6)));
    if !tail {
        x + v * (S1 + z * r)
    } else {
        x - ((z * (0.5 * y - v * r) - y) - v * S1)
    }
}
fn kernel_rem_pio2(x: &[f64], e0: i32) -> (i32, f64, f64) {
    const PIO2: [f64; 8] = [
        f64::from_bits(0x3ff921fb40000000),
        f64::from_bits(0x3e74442d00000000),
        f64::from_bits(0x3cf8469880000000),
        f64::from_bits(0x3b78cc5160000000),
        f64::from_bits(0x39f01b8380000000),
        f64::from_bits(0x387a252040000000),
        f64::from_bits(0x36e3822280000000),
        f64::from_bits(0x3569f31d00000000),
    ];
    const JK: usize = 4;
    let jx = x.len() - 1;
    let jv = ((e0 - 3) / 24).max(0) as usize;
    let mut q0 = e0 - 24 * (jv as i32 + 1);
    let mut f = [0.0; 20];
    let mut fq = [0.0; 20];
    let mut q = [0.0; 20];
    let mut iq = [0_i32; 20];
    for i in 0..=jx + JK {
        let j = jv as i32 - jx as i32 + i as i32;
        f[i] = if j < 0 {
            0.0
        } else {
            f64::from(TWO_OVER_PI[j as usize])
        };
    }
    for i in 0..=JK {
        let mut fw = 0.0;
        for j in 0..=jx {
            fw += x[j] * f[jx + i - j];
        }
        q[i] = fw;
    }
    let mut jz = JK;
    let (n, ih, mut z) = loop {
        let mut z = q[jz];
        for i in 0..jz {
            let j = jz - i;
            let fw = f64::from((TWON24 * z) as i32);
            iq[i] = (z - TWO24 * fw) as i32;
            z = q[j - 1] + fw;
        }
        z = libm::scalbn(z, q0);
        z -= 8.0 * (z * 0.125).floor();
        let mut n = z as i32;
        z -= f64::from(n);
        let mut ih = 0;
        if q0 > 0 {
            let i = iq[jz - 1] >> (24 - q0);
            n += i;
            iq[jz - 1] -= i << (24 - q0);
            ih = iq[jz - 1] >> (23 - q0);
        } else if q0 == 0 {
            ih = iq[jz - 1] >> 23;
        } else if z >= 0.5 {
            ih = 2;
        }
        if ih > 0 {
            n += 1;
            let mut carry = false;
            for i in 0..jz {
                let j = iq[i];
                if !carry {
                    if j != 0 {
                        carry = true;
                        iq[i] = 0x1000000 - j;
                    }
                } else {
                    iq[i] = 0xffffff - j;
                }
            }
            match q0 {
                1 => iq[jz - 1] &= 0x7fffff,
                2 => iq[jz - 1] &= 0x3fffff,
                _ => {}
            }
            if ih == 2 {
                z = 1.0 - z;
                if carry {
                    z -= libm::scalbn(1.0, q0);
                }
            }
        }
        if z == 0.0 && iq[JK..jz].iter().all(|&v| v == 0) {
            let mut k = 1;
            while JK >= k && iq[JK - k] == 0 {
                k += 1;
            }
            for i in jz + 1..=jz + k {
                f[jx + i] = f64::from(TWO_OVER_PI[jv + i]);
                let mut fw = 0.0;
                for j in 0..=jx {
                    fw += x[j] * f[jx + i - j];
                }
                q[i] = fw;
            }
            jz += k;
            continue;
        }
        break (n, ih, z);
    };
    if z == 0.0 {
        jz -= 1;
        q0 -= 24;
        while iq[jz] == 0 {
            jz -= 1;
            q0 -= 24;
        }
    } else {
        z = libm::scalbn(z, -q0);
        if z >= TWO24 {
            let fw = f64::from((TWON24 * z) as i32);
            iq[jz] = (z - TWO24 * fw) as i32;
            jz += 1;
            q0 += 24;
            iq[jz] = fw as i32;
        } else {
            iq[jz] = z as i32;
        }
    }
    let mut fw = libm::scalbn(1.0, q0);
    for i in (0..=jz).rev() {
        q[i] = fw * f64::from(iq[i]);
        fw *= TWON24;
    }
    for i in (0..=jz).rev() {
        let mut fw = 0.0;
        for k in 0..=JK.min(jz - i) {
            fw += PIO2[k] * q[i + k];
        }
        fq[jz - i] = fw;
    }
    let mut fw = 0.0;
    for i in (0..=jz).rev() {
        fw += fq[i];
    }
    let y0 = if ih == 0 { fw } else { -fw };
    fw = fq[0] - fw;
    for i in 1..=jz {
        fw += fq[i];
    }
    let y1 = if ih == 0 { fw } else { -fw };
    (n & 7, y0, y1)
}
fn rem_pio2(x: f64) -> (i32, f64, f64) {
    let hx = high(x);
    let ix = hx & 0x7fffffff;
    let negative = hx & 0x80000000 != 0;
    if ix <= 0x3fe921fb {
        return (0, x, 0.0);
    }
    if ix < 0x4002d97c {
        if !negative {
            let mut z = x - PIO2_1;
            if ix != 0x3ff921fb {
                let y0 = z - PIO2_1T;
                return (1, y0, (z - y0) - PIO2_1T);
            }
            z -= PIO2_2;
            let y0 = z - PIO2_2T;
            return (1, y0, (z - y0) - PIO2_2T);
        }
        let mut z = x + PIO2_1;
        if ix != 0x3ff921fb {
            let y0 = z + PIO2_1T;
            return (-1, y0, (z - y0) + PIO2_1T);
        }
        z += PIO2_2;
        let y0 = z + PIO2_2T;
        return (-1, y0, (z - y0) + PIO2_2T);
    }
    if ix <= 0x413921fb {
        let t = x.abs();
        let n = (t * INVPIO2 + 0.5) as i32;
        let nf = f64::from(n);
        let mut r = t - nf * PIO2_1;
        let mut w = nf * PIO2_1T;
        let mut y0 = r - w;
        if !(n < 32 && ix != NPIO2_HW[n as usize - 1]) {
            let j = ix >> 20;
            let i = j - ((high(y0) >> 20) & 0x7ff);
            if i > 16 {
                let t = r;
                w = nf * PIO2_2;
                r = t - w;
                w = nf * PIO2_2T - ((t - r) - w);
                y0 = r - w;
                let i = j - ((high(y0) >> 20) & 0x7ff);
                if i > 49 {
                    let t = r;
                    w = nf * PIO2_3;
                    r = t - w;
                    w = nf * PIO2_3T - ((t - r) - w);
                    y0 = r - w;
                }
            }
        }
        let y1 = (r - y0) - w;
        return if negative {
            (-n, -y0, -y1)
        } else {
            (n, y0, y1)
        };
    }
    if ix >= 0x7ff00000 {
        return (0, f64::NAN, f64::NAN);
    }
    let e0 = (ix >> 20) as i32 - 1046;
    let high_word = ix.wrapping_sub((e0 as u32) << 20);
    let mut z = f64::from_bits((u64::from(high_word) << 32) | (x.to_bits() & 0xffffffff));
    let mut tx = [0.0; 3];
    for i in 0..2 {
        tx[i] = f64::from(z as i32);
        z = (z - tx[i]) * TWO24;
    }
    tx[2] = z;
    let mut nx = 3;
    while tx[nx - 1] == 0.0 {
        nx -= 1;
    }
    let (n, y0, y1) = kernel_rem_pio2(&tx[..nx], e0);
    if negative {
        (-n, -y0, -y1)
    } else {
        (n, y0, y1)
    }
}
pub fn sin(x: f64) -> f64 {
    let ix = high(x) & 0x7fffffff;
    if ix <= 0x3fe921fb {
        return kernel_sin(x, 0.0, false);
    }
    if ix >= 0x7ff00000 {
        return f64::NAN;
    }
    let (n, y0, y1) = rem_pio2(x);
    match n & 3 {
        0 => kernel_sin(y0, y1, true),
        1 => kernel_cos(y0, y1),
        2 => -kernel_sin(y0, y1, true),
        _ => -kernel_cos(y0, y1),
    }
}
pub fn cos(x: f64) -> f64 {
    let ix = high(x) & 0x7fffffff;
    if ix <= 0x3fe921fb {
        return kernel_cos(x, 0.0);
    }
    if ix >= 0x7ff00000 {
        return f64::NAN;
    }
    let (n, y0, y1) = rem_pio2(x);
    match n & 3 {
        0 => kernel_cos(y0, y1),
        1 => -kernel_sin(y0, y1, true),
        2 => -kernel_cos(y0, y1),
        _ => kernel_sin(y0, y1, true),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn frozen_node_trig_bits_include_large_reduction_and_signed_zero() {
        // Measured with the project Node 24.22.1, V8 13.6.233.17-node.58.
        for (x, sine, cosine) in [
            (0, 0, 0x3ff0000000000000),
            (0x8000000000000000, 0x8000000000000000, 0x3ff0000000000000),
            (0x3fe668f791899724, 0x3fe49f64f99f0207, 0x3fe877eee269d586),
            (0x41a4ce527a661c13, 0xbfef838b8e3846f7, 0x3fc63a1a5720e16e),
            (0x54546d7b9107c3ec, 0x3fdf49d30e23f552, 0x3febea4bb289a493),
            (0x7ddaafe6d51317ba, 0x3feff19e54b0b412, 0xbfae53373fcbbb70),
        ] {
            assert_eq!(sin(f64::from_bits(x)).to_bits(), sine, "sin {x:x}");
            assert_eq!(cos(f64::from_bits(x)).to_bits(), cosine, "cos {x:x}");
        }
    }
}
