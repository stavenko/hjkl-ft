//! Правила отбора миграций — без базы и без сети: решает `should_run`, его и
//! проверяем.

use super::{should_run, UNMIGRATED};

#[test]
fn nulevaya_migraciya_idet_kogda_versii_net_voobshche() {
    assert!(should_run(0, UNMIGRATED));
}

#[test]
fn migraciya_idet_kogda_versiya_bazy_menshe() {
    // На базу версии 4 накатывается скрипт 5.
    assert!(should_run(5, 4));
}

#[test]
fn migraciya_propuskaetsya_kogda_baza_uzhe_novee() {
    // Скрипт 5 на базе версии 6 игнорируется.
    assert!(!should_run(5, 6));
}

#[test]
fn migraciya_propuskaetsya_kogda_versii_ravny() {
    // Прогон дважды не должен повторять работу.
    assert!(!should_run(3, 3));
}

#[test]
fn otstavshaya_baza_dogonyaet_vse_propushchennye() {
    // База версии 0, миграций пять — пойдут все, кроме нулевой.
    let base = 0i64;
    let would_run: Vec<u32> = (0..5).filter(|v| should_run(*v, base)).collect();
    assert_eq!(would_run, vec![1, 2, 3, 4]);
}

#[test]
fn versii_migracii_idut_s_nulya_podryad() {
    // То же условие, что проверяет `run_all` в debug: нумерация с нуля, без
    // пропусков и повторов. Здесь оно закреплено тестом, а не только assert'ом.
    let versions: Vec<u32> = super::ALL.iter().map(|m| m.version).collect();
    assert_eq!(versions.first(), Some(&0), "нумерация начинается с нуля");
    for pair in versions.windows(2) {
        assert_eq!(pair[1], pair[0] + 1, "версии идут подряд: {pair:?}");
    }
}

#[test]
fn u_kazhdoy_migracii_est_opisanie() {
    for m in super::ALL {
        assert!(!m.description.trim().is_empty(), "миграция {} без описания", m.version);
    }
}
