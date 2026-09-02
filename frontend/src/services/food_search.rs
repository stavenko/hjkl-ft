//! Поиск еды в базе человека: отбор кандидатов и схлопывание почти-повторов.
//!
//! Здесь нет ни модели, ни сети — только текст и арифметика. Всё, что решает этот
//! модуль, проверяется точно, потому что от него зависит, заведётся ли у человека
//! вторая копия того же продукта.
//!
//! Числа и правила ниже не выбраны по вкусу, а получены замерами
//! (`scripts/measure-food-search.mjs`, `scripts/measure-search-dedup.mjs`) на
//! каталоге из 45 продуктов и 24 запросов:
//!
//!   подстрока (как искали раньше)   3 из 20
//!   по словам, обрубок 4 буквы      17 из 20
//!   по ключевым словам              20 из 20
//!
//! Подстрока проваливается не по мелочи: «ракушки» и «макароны» не имеют общих
//! букв, и связать их сравнением строк нельзя в принципе — слово просто другое.
//! Поэтому у каждого продукта есть `keywords`, размеченные один раз при заведении.

use std::collections::{BTreeMap, BTreeSet};

use api_types::Food;

/// Привести слово к сравнимому виду: регистр, ё, и прочь всё, что не буква и не
/// цифра. Пробелы убираются целиком — «ма кароны» это те же макароны с промахом по
/// клавише, а не другой продукт.
pub fn canon(s: &str) -> String {
    s.to_lowercase()
        .chars()
        .map(|c| if c == 'ё' { 'е' } else { c })
        .filter(|c| c.is_alphanumeric())
        .collect()
}

/// Грубая основа слова: русский словоизменяет хвостом, и четырёх букв хватает,
/// чтобы «огурец» сошёлся с «огурцами», а «гречневая» с «гречкой».
///
/// Четыре, а не пять: на пяти эти две пары расходятся, и находилось 17 из 20 вместо
/// 20. На трёх список кандидатов растёт без пользы.
const STEM_LEN: usize = 4;

fn stem(word: &str) -> String {
    word.chars().take(STEM_LEN).collect()
}

/// Основы всех слов строки. Слова короче трёх букв отбрасываются: предлоги и
/// обрывки сшивают между собой что угодно.
pub fn stems(s: &str) -> BTreeSet<String> {
    s.to_lowercase()
        .chars()
        .map(|c| if c == 'ё' { 'е' } else { c })
        .map(|c| if c.is_alphanumeric() { c } else { ' ' })
        .collect::<String>()
        .split_whitespace()
        .filter(|w| w.chars().count() >= 3)
        .map(stem)
        .collect()
}

/// Наибольшее число ключевых слов у одного продукта. Индекс должен быть конечным, а
/// хвост длинного списка ничего не добавляет.
const MAX_KEYWORDS: usize = 12;

/// Привести список ключевых слов в годный для индекса вид.
///
/// Модель приносит его сырым, и в нём три постоянные болячки, все три видны на наших
/// же замерах: повторы (у творога «сухой творог» пришёл пятнадцать раз — та же
/// вырожденная петля, что и в расшифровках этикеток), выдуманные слова
/// («барабарки», «литтре») и голые числа («15» из «15%»).
///
/// Повторы и числа выбрасываем: первое раздувает индекс, второе слепляет всё, где
/// есть та же цифра. Выдуманные оставляем — отличить их от редкого настоящего
/// названия нечем, а вреда нет: они ни с чем не сойдутся.
pub fn clean_keywords(list: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    let mut seen = BTreeSet::new();
    for raw in list {
        let w: String = raw
            .trim()
            .to_lowercase()
            .chars()
            .map(|c| if c == 'ё' { 'е' } else { c })
            .collect();
        if w.chars().count() < 3 || !w.chars().any(|c| c.is_alphabetic()) {
            continue;
        }
        if !seen.insert(w.clone()) {
            continue;
        }
        out.push(w);
        if out.len() >= MAX_KEYWORDS {
            break;
        }
    }
    out
}

/// Обратный индекс «основа слова → продукты». Строится один раз при загрузке базы,
/// дальше поиск это несколько взятий по ключу, а не проход по каталогу.
///
/// На сорока пяти продуктах разницы не видно, но сопоставление зовётся на КАЖДЫЙ
/// распознанный продукт, а база растёт только в одну сторону: на каталоге в двадцать
/// тысяч перебор занимал 50 мс против 0.26 мс у индекса.
#[derive(Debug, Default)]
pub struct Index {
    by_stem: BTreeMap<String, BTreeSet<String>>,
}

impl Index {
    pub fn build(foods: &[Food]) -> Self {
        let mut by_stem: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        for f in foods {
            // Ищем и по названию, и по размеченным словам: у еды, заведённой до
            // разметки, слов нет, и без названия она бы не находилась вовсе.
            let mut all = stems(&f.name);
            for w in clean_keywords(&f.keywords) {
                all.extend(stems(&w));
            }
            for st in all {
                by_stem.entry(st).or_default().insert(f.id.clone());
            }
        }
        Self { by_stem }
    }

    /// Кандидаты по запросу и его ключевым словам. Список НАМЕРЕННО с запасом:
    /// лишний кандидат стоит одной строки в промпте, а упущенный — второй копии
    /// продукта в базе навсегда.
    pub fn candidates(&self, query: &str, query_keywords: &[String]) -> BTreeSet<String> {
        let mut wanted = stems(query);
        for w in clean_keywords(query_keywords) {
            wanted.extend(stems(&w));
        }
        let mut hits = BTreeSet::new();
        for st in &wanted {
            if let Some(ids) = self.by_stem.get(st) {
                hits.extend(ids.iter().cloned());
            }
        }
        hits
    }

    pub fn len(&self) -> usize {
        self.by_stem.len()
    }

    pub fn is_empty(&self) -> bool {
        self.by_stem.is_empty()
    }
}

// ── почти-повторы ────────────────────────────────────────────────────────────

/// Расстояние Левенштейна — сколько правок отделяет одно написание от другого.
fn distance(a: &str, b: &str) -> usize {
    if a == b {
        return 0;
    }
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    for i in 1..=a.len() {
        let mut cur = vec![i];
        for j in 1..=b.len() {
            let sub = prev[j - 1] + usize::from(a[i - 1] != b[j - 1]);
            cur.push((prev[j] + 1).min(cur[j - 1] + 1).min(sub));
        }
        prev = cur;
    }
    prev[b.len()]
}

/// Одно и то же написание с точностью до описки.
///
/// Короткие слова не трогаем вовсе: «мясо» и «масло» отличаются на две правки из
/// пяти букв, а это совершенно разная еда. Уменьшительные («макарошки») сюда не
/// попадают — там правок больше; это вопрос к модели, а не к строкам.
pub fn same_name(a: &str, b: &str) -> bool {
    let (x, y) = (canon(a), canon(b));
    if x.is_empty() || y.is_empty() {
        return false;
    }
    if x == y {
        return true;
    }
    let len = x.chars().count().min(y.chars().count());
    if len < 6 {
        return false;
    }
    distance(&x, &y) <= if len >= 10 { 2 } else { 1 }
}

/// Совпадают ли нутриенты с точностью до округления на упаковке.
pub fn same_nutrition(a: &Food, b: &Food) -> bool {
    let pairs: [(f64, f64, f64); 4] = [
        (a.kcal, b.kcal, 1.0),
        (a.protein, b.protein, 0.1),
        (a.fat, b.fat, 0.1),
        (a.carbs, b.carbs, 0.1),
    ];
    pairs
        .iter()
        .all(|(x, y, floor)| (x - y).abs() <= floor.max(y.abs() * 0.01))
}

/// Схлопнуть неразличимые для человека записи, оставив первую из каждой группы.
///
/// Требуется И имя, И числа. Хватило бы одного имени — склеились бы макароны с
/// разной калорийностью, а по спеке отличие в числах и есть отдельный продукт.
/// Хватило бы одних чисел — склеились бы макароны с вермишелью, которые человек
/// развёл нарочно и которые его устраивают.
pub fn collapse_duplicates(foods: &[Food]) -> Vec<Food> {
    let mut out: Vec<Food> = Vec::new();
    for f in foods {
        if out
            .iter()
            .any(|g| same_name(&g.name, &f.name) && same_nutrition(g, f))
        {
            continue;
        }
        out.push(f.clone());
    }
    out
}

// ── арифметический отбор перед выбором ───────────────────────────────────────

/// Прочитанные с этикетки нутриенты. `None` там, где прочесть не удалось: с одного
/// кадра сведений почти всегда не хватает.
#[derive(Debug, Clone, Copy, Default)]
pub struct SeenNutrition {
    pub kcal: Option<f64>,
    pub protein: Option<f64>,
    pub fat: Option<f64>,
    pub carbs: Option<f64>,
}

impl SeenNutrition {
    pub fn all_four_read(&self) -> bool {
        self.kcal.is_some() && self.protein.is_some() && self.fat.is_some() && self.carbs.is_some()
    }
}

/// Совпадают ли числа ПО ТЕМ параметрам, которые удалось прочитать.
///
/// `None` — сравнивать нечем, и код молчит: решать будет модель по имени.
/// `Some(false)` — расходятся, и по спеке (§6.4) это НОВАЯ копия продукта, а не та
/// же самая. Спрашивать об этом модель нельзя: на замере она соглашалась считать
/// творог 110/17/3.0 тем же, что лежащий в базе 96/18/1.2, потому что названия
/// совпадают. Цена такого согласия — чужое КБЖУ в дневнике.
pub fn numbers_agree(seen: &SeenNutrition, food: &Food) -> Option<bool> {
    let pairs: [(Option<f64>, f64, f64, f64); 4] = [
        (seen.kcal, food.kcal, 1.0, 0.01),
        (seen.protein, food.protein, 0.1, 0.05),
        (seen.fat, food.fat, 0.1, 0.05),
        (seen.carbs, food.carbs, 0.1, 0.05),
    ];
    let mut compared = 0;
    for (got, want, floor, rel) in pairs {
        let Some(got) = got else { continue };
        compared += 1;
        if (got - want).abs() > floor.max(want.abs() * rel) {
            return Some(false);
        }
    }
    (compared > 0).then_some(true)
}

/// Кандидаты, пережившие арифметику: те, чьи прочитанные числа не противоречат.
pub fn survivors<'a>(seen: &SeenNutrition, pool: &'a [Food]) -> Vec<&'a Food> {
    pool.iter()
        .filter(|f| numbers_agree(seen, f) != Some(false))
        .collect()
}

/// Решить БЕЗ модели, если это возможно. Возвращает `id` найденного продукта.
///
/// Два случая, и оба замерены. Первый: то же написание с точностью до описки —
/// «Макароны», «макароны», «Ма кароны», «Макарони». Второй: все четыре нутриента
/// прочитаны, все четыре сошлись, кандидат остался один; спека называет это полной
/// копией по КБЖУ. На замере обращение к модели в этом случае раз из трёх давало
/// отказ — то есть лишнюю копию продукта на ровном месте.
pub fn decide_without_model(seen: &SeenNutrition, name: &str, pool: &[&Food]) -> Option<String> {
    if let Some(f) = pool.iter().find(|f| same_name(name, &f.name)) {
        return Some(f.id.clone());
    }
    match pool {
        [only] if seen.all_four_read() && numbers_agree(seen, only) == Some(true) => {
            Some(only.id.clone())
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn food(id: &str, name: &str, kcal: f64, p: f64, f: f64, c: f64) -> Food {
        Food {
            id: id.into(),
            name: name.into(),
            kcal,
            protein: p,
            fat: f,
            carbs: c,
            ..Default::default()
        }
    }

    #[test]
    fn stem_length_four_joins_the_pairs_that_five_split() {
        // На пяти буквах «огурец»/«огурцы» и «гречневая»/«гречка» расходились, и
        // находилось 17 из 20 вместо 20. Эти две пары и есть весь спор о длине.
        assert_eq!(stem("огурец"), stem("огурцы"));
        assert_eq!(stem("гречневая"), stem("гречка"));
        assert_ne!(stem("макароны"), stem("молоко"));
    }

    #[test]
    fn short_words_are_dropped_from_stems() {
        let s = stems("суп из огурцов и трав");
        assert!(s.contains("огур"));
        assert!(!s.iter().any(|w| w == "из" || w == "и"));
    }

    #[test]
    fn keywords_are_cleaned_of_repeats_and_bare_numbers() {
        let raw = vec![
            "сухой творог".to_string(),
            "СУХОЙ ТВОРОГ".to_string(), // повтор в другом регистре
            "15".to_string(),           // голое число из «15%»
            "тв".to_string(),           // обрывок
            "творог".to_string(),
        ];
        assert_eq!(clean_keywords(&raw), vec!["сухой творог", "творог"]);
    }

    #[test]
    fn keyword_list_is_bounded() {
        let raw: Vec<String> = (0..40).map(|i| format!("слово{i}")).collect();
        assert_eq!(clean_keywords(&raw).len(), MAX_KEYWORDS);
    }

    #[test]
    fn index_finds_by_keyword_where_substring_never_could() {
        // Ровно тот случай, ради которого всё и затевалось: «ракушки» и «макароны»
        // не имеют общих букв, и подстрока их не свяжет никогда.
        let mut pasta = food("f01", "Макароны", 337.0, 10.4, 1.1, 71.5);
        pasta.keywords = vec!["макароны".into(), "паста".into(), "ракушки".into()];
        let milk = food("f02", "Молоко 2.5%", 52.0, 2.9, 2.5, 4.7);
        let idx = Index::build(&[pasta, milk]);

        assert!(!"макароны".contains("ракушки"));
        let hits = idx.candidates("Ракушки", &[]);
        assert!(hits.contains("f01"), "ключевое слово должно находить продукт");
        assert!(!hits.contains("f02"));
    }

    #[test]
    fn index_still_finds_food_without_keywords() {
        // Еда, заведённая до разметки, обязана находиться по названию.
        let idx = Index::build(&[food("f03", "Гречка варёная", 110.0, 4.2, 1.1, 21.3)]);
        assert!(idx.candidates("гречневая каша", &[]).contains("f03"));
    }

    #[test]
    fn same_name_catches_typos_but_not_short_lookalikes() {
        assert!(same_name("Макароны", "макароны"));
        assert!(same_name("Ма кароны", "Макароны"));
        assert!(same_name("Макарони", "Макароны"));
        assert!(same_name("Творог 5%", "творог 5 %"));
        // Разная еда, похожие буквы: две правки из пяти.
        assert!(!same_name("Мясо", "Масло"));
        assert!(!same_name("Сельдь", "Сельдерей"));
        assert!(!same_name("Сыр", "Сырок"));
        assert!(!same_name("Курица", "Куркума"));
        // Уменьшительное кодом не берётся — это к модели.
        assert!(!same_name("Макароны", "Макарошки"));
    }

    #[test]
    fn duplicates_collapse_only_when_name_and_numbers_both_match() {
        let foods = vec![
            food("d01", "Макароны", 337.0, 10.4, 1.1, 71.5),
            food("d02", "макароны", 337.0, 10.4, 1.1, 71.5),
            food("d03", "Ма кароны", 337.0, 10.4, 1.1, 71.5),
            // То же слово, ДРУГИЕ числа: по спеке отдельный продукт.
            food("d04", "Макароны", 350.0, 11.0, 1.3, 70.0),
            // Те же числа, ДРУГОЕ слово: человек развёл их нарочно.
            food("d05", "Вермишель", 337.0, 10.4, 1.1, 71.5),
        ];
        let left = collapse_duplicates(&foods);
        let ids: Vec<&str> = left.iter().map(|f| f.id.as_str()).collect();
        assert_eq!(ids, vec!["d01", "d04", "d05"]);
    }

    #[test]
    fn numbers_that_differ_rule_the_candidate_out() {
        let in_base = food("f08", "Творог обезжиренный", 96.0, 18.0, 1.2, 3.3);
        let same = SeenNutrition { kcal: Some(96.0), protein: Some(18.0), fat: Some(1.2), carbs: Some(3.3) };
        let fatter = SeenNutrition { kcal: Some(110.0), protein: Some(17.0), fat: Some(3.0), carbs: Some(3.3) };
        assert_eq!(numbers_agree(&same, &in_base), Some(true));
        assert_eq!(numbers_agree(&fatter, &in_base), Some(false));
        // Ничего не прочитано — сравнивать нечем, решает модель.
        assert_eq!(numbers_agree(&SeenNutrition::default(), &in_base), None);
    }

    #[test]
    fn partial_reading_compares_only_what_was_read() {
        let in_base = food("f08", "Творог обезжиренный", 96.0, 18.0, 1.2, 3.3);
        let partial = SeenNutrition { kcal: Some(96.0), protein: Some(18.0), ..Default::default() };
        assert_eq!(numbers_agree(&partial, &in_base), Some(true));
        assert!(!partial.all_four_read());
    }

    #[test]
    fn code_decides_the_full_copy_and_the_typo_without_asking_the_model() {
        let pack = food("f08", "Творог обезжиренный ВкусВилл", 96.0, 18.0, 1.2, 3.3);
        let pool = vec![&pack];
        let seen = SeenNutrition { kcal: Some(96.0), protein: Some(18.0), fat: Some(1.2), carbs: Some(3.3) };
        // Все четыре сошлись, кандидат один — полная копия по КБЖУ.
        assert_eq!(decide_without_model(&seen, "Творог «Пластовой»", &pool), Some("f08".into()));

        let pasta = food("f01", "Макароны", 337.0, 10.4, 1.1, 71.5);
        let pool = vec![&pasta];
        // То же написание с точностью до пробела — решается именем.
        assert_eq!(
            decide_without_model(&SeenNutrition::default(), "Ма кароны", &pool),
            Some("f01".into())
        );
    }

    #[test]
    fn code_does_not_decide_when_it_should_not() {
        let a = food("f01", "Макароны", 337.0, 10.4, 1.1, 71.5);
        let b = food("f02", "Спагетти Barilla №5", 359.0, 12.0, 1.5, 71.2);
        let pool = vec![&a, &b];
        // Кандидатов двое — полного совпадения по числам недостаточно.
        let seen = SeenNutrition { kcal: Some(337.0), protein: Some(10.4), fat: Some(1.1), carbs: Some(71.5) };
        assert_eq!(decide_without_model(&seen, "Ракушки", &pool), None);
        // Один кандидат, но прочитано не всё — решать модели.
        let pool = vec![&a];
        let partial = SeenNutrition { kcal: Some(337.0), ..Default::default() };
        assert_eq!(decide_without_model(&partial, "Ракушки", &pool), None);
    }
}
