//! Des verites geometriques, pas des comptes d'appels.

use super::*;
use crate::text::{Font, TextCache, tests::Boxes};

fn alpha_sum(s: &Surface, r: IRect) -> f64 {
    let mut t = 0.0;
    for y in r.y..r.bottom() {
        for x in r.x..r.right() {
            t += (s.get(x, y) >> 24) as f64 / 255.0;
        }
    }
    t
}

#[test]
fn le_decoupage_est_au_pixel_pour_toutes_les_primitives() {
    let mut s = Surface::new(60, 60);
    let mut k = Scratch::default();
    {
        let mut cv = Canvas::new(&mut s, &mut k);
        cv.push_clip(20.0, 20.0, 20.0, 20.0);
        cv.fill_rect(0.0, 0.0, 60.0, 60.0, rgb(255, 0, 0));
        cv.fill_circle(30.0, 30.0, 40.0, rgb(0, 255, 0));
        cv.line(0.0, 0.0, 60.0, 60.0, 5.0, rgb(0, 0, 255));
        cv.fill_round_rect(-10.0, -10.0, 80.0, 80.0, 12.0, rgb(9, 9, 9));
        let img = Surface {
            w: 60,
            h: 60,
            px: vec![rgb(1, 2, 3).premul(); 3600],
        };
        cv.draw_image(&img, 0.0, 0.0, 1.0);
        cv.draw_mask(&[255; 3600], 60, 60, 0, 0, rgb(4, 5, 6));
        cv.pop_clip();
    }
    for y in 0..60 {
        for x in 0..60 {
            let inside = (20..40).contains(&x) && (20..40).contains(&y);
            assert_eq!(s.get(x, y) != 0, inside, "pixel {x},{y}");
        }
    }
}

#[test]
fn un_rectangle_fractionnaire_depose_son_aire() {
    let mut s = Surface::new(40, 40);
    let mut k = Scratch::default();
    {
        let mut cv = Canvas::new(&mut s, &mut k);
        cv.fill_rect(3.25, 4.5, 10.5, 7.75, rgb(255, 255, 255));
    }
    let a = alpha_sum(&s, IRect::new(0, 0, 40, 40));
    assert!((a - 10.5 * 7.75).abs() < 0.05, "{a}");
}

#[test]
fn un_disque_rempli_vaut_pi_r_carre_en_pixels() {
    let mut s = Surface::new(80, 80);
    let mut k = Scratch::default();
    {
        let mut cv = Canvas::new(&mut s, &mut k);
        cv.fill_circle(40.3, 39.6, 25.0, rgb(255, 255, 255));
    }
    let a = alpha_sum(&s, IRect::new(0, 0, 80, 80));
    let want = std::f64::consts::PI * 625.0;
    assert!((a - want).abs() / want < 3e-3, "{a} contre {want}");
}

#[test]
fn l_origine_deplace_tout() {
    let mut a = Surface::new(50, 50);
    let mut b = Surface::new(50, 50);
    let mut k = Scratch::default();
    {
        let mut cv = Canvas::new(&mut a, &mut k);
        cv.fill_round_rect(12.0, 13.0, 20.0, 15.0, 4.0, rgb(200, 100, 50));
    }
    {
        let mut cv = Canvas::new(&mut b, &mut k);
        cv.push_origin(10.0, 10.0);
        cv.fill_round_rect(2.0, 3.0, 20.0, 15.0, 4.0, rgb(200, 100, 50));
        cv.pop_origin();
    }
    assert_eq!(a.px, b.px);
}

#[test]
fn un_rectangle_arrondi_a_l_aire_attendue() {
    let mut s = Surface::new(100, 100);
    let mut k = Scratch::default();
    {
        let mut cv = Canvas::new(&mut s, &mut k);
        cv.fill_round_rect(10.0, 10.0, 60.0, 40.0, 8.0, rgb(255, 255, 255));
    }
    let a = alpha_sum(&s, IRect::new(0, 0, 100, 100));
    let want = 60.0 * 40.0 - (4.0 - std::f64::consts::PI) * 64.0;
    assert!((a - want).abs() / want < 2e-3, "{a} contre {want}");
}

/// Une planche de tout ce que le moteur sait faire, ecrite en PNG pour etre
/// REGARDEE (`%TEMP%\cp_gfx_planche.png`).
#[test]
fn planche() {
    let (w, h) = (640, 360);
    let mut s = Surface::new(w, h);
    let mut k = Scratch::default();
    let mut t = TextCache::new(Box::new(Boxes));
    {
        let mut cv = Canvas::new(&mut s, &mut k);
        cv.clear(hex(0x1C1C20));
        // Des rectangles arrondis de toutes tailles, a des positions
        // fractionnaires.
        for i in 0..8 {
            let x = 20.0 + i as f32 * 74.3;
            cv.fill_round_rect(x, 20.0, 64.0, 40.0, i as f32 * 3.0, hex(0x3A6EA5));
            cv.stroke_round_rect(x, 70.0, 64.0, 40.0, i as f32 * 3.0, 1.5, hex(0xE0E0E8));
        }
        // Disques et anneaux.
        for i in 0..8 {
            let x = 52.0 + i as f32 * 74.3;
            cv.fill_circle(x, 150.0, 4.0 + i as f32 * 3.0, hex(0xD08040));
            cv.stroke_circle(x, 215.0, 26.0, 1.0 + i as f32, hex(0x60C080));
        }
        // Traits a tous les angles.
        for i in 0..24 {
            let a = i as f32 / 24.0 * std::f32::consts::TAU;
            cv.line(
                110.0,
                300.0,
                110.0 + 50.0 * a.cos(),
                300.0 + 50.0 * a.sin(),
                1.0 + (i % 4) as f32 * 0.75,
                hex(0xC8C8D0),
            );
        }
        // Une courbe de Bezier epaisse et une forme de forme d'onde.
        let mut p = Path::new();
        p.move_to(200.0, 330.0)
            .cubic_to(240.0, 240.0, 300.0, 380.0, 340.0, 260.0);
        cv.stroke_path(&p, 3.0, hex(0xF0C040));
        for x in 0..250 {
            let v = ((x as f32 * 0.21).sin() * (x as f32 * 0.013).cos()) * 40.0;
            cv.fill_rect(
                370.0 + x as f32,
                300.0 - v.abs(),
                1.0,
                2.0 * v.abs() + 1.0,
                hex(0x5AA0E0),
            );
        }
        // Degrades.
        cv.fill_rect(
            200.0,
            130.0,
            150.0,
            40.0,
            Paint::Linear {
                x0: 200.0,
                y0: 0.0,
                x1: 350.0,
                y1: 0.0,
                c0: hex(0x202040),
                c1: hex(0xE06080),
            },
        );
        // Transparence et decoupage.
        cv.push_clip(400.0, 130.0, 100.0, 60.0);
        cv.fill_circle(430.0, 160.0, 40.0, rgba(255, 0, 0, 128));
        cv.fill_circle(470.0, 160.0, 40.0, rgba(0, 0, 255, 128));
        cv.pop_clip();
        t.draw(
            &mut cv,
            Font::new(0, 14, 400),
            520.0,
            160.0,
            "Ab c",
            hex(0xFFFFFF),
        );
    }
    let path = std::env::temp_dir().join("cp_gfx_planche.png");
    std::fs::write(&path, png::encode(&s)).unwrap();
}

#[test]
fn seules_les_bandes_changees_partent_a_l_ecran() {
    let (w, h) = (4usize, 10usize);
    let mut shown = vec![0u32; w * h];
    let mut now = shown.clone();
    assert!(changed_bands(&now, &mut shown, w, 0, h, 2).is_empty());
    now[w + 1] = 7; // ligne 1
    now[2 * w] = 7; // ligne 2
    now[7 * w + 3] = 9; // ligne 7
    let b = changed_bands(&now, &mut shown, w, 0, h, 2);
    assert_eq!(b, vec![(1, 3), (7, 8)]);
    assert_eq!(shown, now, "l'ecran retenu suit");
    // Deux bandes proches se fondent.
    now[4 * w] = 1;
    now[6 * w] = 1;
    assert_eq!(changed_bands(&now, &mut shown, w, 0, h, 3), vec![(4, 7)]);
    // La derniere ligne.
    now[9 * w] = 5;
    assert_eq!(changed_bands(&now, &mut shown, w, 0, h, 2), vec![(9, 10)]);
}
