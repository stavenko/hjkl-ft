// ПРОДУКТЫ, ПОРОГИ И СЧЁТ КОРЗИНЫ — общий модуль.
//
// Одни и те же числа нужны в двух местах: в `cheap-basket.mjs` (подбор корзины
// руками) и в `price-basket-page.mjs` (пересчёт таблиц на странице корзины).
// Разъедутся — страница начнёт врать, поэтому источник один.
//
// Цены — ₽/кг, пересчитанные из ФАКТИЧЕСКИХ карточек Ozon (Санкт-Петербург,
// 25 августа 2026). У каждой позиции в поле `pack` записано, из чего цена взялась:
// бренд, наименование, фасовка и цена упаковки. Берётся цена карточки на тот день
// (та, что показана первой, — со скидкой Ozon); зачёркнутая «старая» не в счёт.
// Позиции с ценой без `pack` в корзине не участвуют и по Ozon не проверялись.
//
// ЖИР МОЛОЧКИ В БАЛАНС НЕ ИДЁТ. У кефира и творога он заключён в целую
// молочно-жировую глобулу, и приложение исключает такой жир из отношения
// ненасыщенных к насыщенным (`local.rs`, ветка `is_milk_globule`). Считать иначе —
// значит записать молочку во вредное и гнать человека на обезжиренное без причины.

// ── Пороги приложения ───────────────────────────────────────────────────────
export const RULES = {
  vegPerDayM: 800,            // indicators::veg_fruit_per_day_g (мужчины)
  vegPerDayF: 600,            // то же, женщины
  calciumPerDay: 1000,        // indicators::CALCIUM_PER_DAY_MG
  fiberPer1000: 14,           // fiber::G_PER_1000_KCAL
  fiberMinPerDay: 25,         // fiber::MIN_G_PER_DAY
  hemePortionsWeek: 3,        // heme::WEEKLY_PORTIONS
  hemePortionProtein: 25,     // heme::PORTION_PROTEIN_G
  epaDhaWeek: 1.75,           // fats::EPA_DHA_PER_WEEK_G
  unsatToSatMin: 2.0,         // fats::UNSAT_TO_SAT_MIN
  redMeatWeek: 700,           // red_meat::WEEKLY_LIMIT_RAW_G (сырой эквивалент)
  eggGramsWeek: 350,          // egg::weekly_min_grams()
  ironBioavailability: 0.18,  // iron::RDA_BIOAVAILABILITY
};

/// Планка по белку — кривая profile::protein_from_kcal, зажатая телом.
export function proteinTarget(kcal, weightKg, heightCm, age, sex) {
  const ANCHOR_KCAL = 1800, ANCHOR_G = 135, K = -0.5850;
  const base = kcal <= ANCHOR_KCAL
    ? ANCHOR_G * kcal / ANCHOR_KCAL
    : ANCHOR_G * Math.pow(kcal / ANCHOR_KCAL, 1 + K);
  // Deurenberg: BF% = 1.20·BMI + 0.23·age − 10.8·(муж=1) − 5.4
  const bmi = weightKg / Math.pow(heightCm / 100, 2);
  const bf = 1.20 * bmi + 0.23 * age - 10.8 * (sex === "male" ? 1 : 0) - 5.4;
  const ffm = weightKg * (1 - bf / 100);
  return Math.round(Math.min(Math.max(base, 1.6 * ffm), 2.2 * weightKg));
}

/// Недельная планка усвоенного железа: суточная основа × 7 × 0.18.
export function ironWeekTarget(sex, age) {
  const menstruating = sex !== "male" && age >= 19 && age <= 50;
  const basis = menstruating ? 8.1 : (sex === "male" ? 8.0 : 8.0);
  return basis * 7 * RULES.ironBioavailability;
}

// ── Продукты: состав на 100 г и цена ────────────────────────────────────────
// kcal, protein, fat, carbs, fibre, calcium (мг), iron (мг), absorption (доля),
// sfa/mufa/pufa/epa_dha — доли ОТ ЖИРА, %; heme/veg/egg/red/processed — признаки.
const F = (name, price, o) => ({
  name, price, pack: null, photo: null, photoFit: "cover", kcal: 0, protein: 0, fat: 0, fibre: 0, calcium: 0, iron: 0,
  absorption: 0.05, sfa: 0, mufa: 0, pufa: 0, epa: 0,
  heme: false, veg: false, egg: false, red: false, globule: false, ...o,
});

export const FOODS = {
  // Белок. Бедро дешевле грудки и жирнее — жир здесь союзник: он идёт в баланс.
  chickenThigh: F("Куриное бедро (без кости)", 532,
    { pack: { brand: "Ярославский бройлер", item: "бедро цыплёнка-бройлера", size: "750 г", price: 399 },
      photo: "chicken",
      kcal: 185, protein: 19, fat: 12, calcium: 12, iron: 1.3, absorption: 0.2,
      sfa: 28, mufa: 45, pufa: 22, heme: true }),
  chickenBreast: F("Куриная грудка", 572,
    { pack: { brand: "Первая свежесть", item: "грудка цыплёнка-бройлера", size: "750 г", price: 429 },
      photo: "chicken",
      kcal: 113, protein: 23.6, fat: 1.9, calcium: 10, iron: 1.0, absorption: 0.2,
      sfa: 28, mufa: 36, pufa: 21, heme: true }),
  // Печень — самый дешёвый гем и железо разом.
  chickenLiver: F("Печень куриная", 411,
    { pack: { brand: "Первая Свежесть", item: "печень куриная охлаждённая", size: "450 г", price: 185 },
      photo: "liver",
      kcal: 137, protein: 20.4, fat: 5.9, calcium: 11, iron: 8.6, absorption: 0.25,
      sfa: 33, mufa: 27, pufa: 24, heme: true }),
  // Мидии варёно-мороженые — гем и цинк, но железа меньше печени.
  mussels: F("Мидии варёно-мороженые", 600,
    { kcal: 86, protein: 24, fat: 2.2, calcium: 33, iron: 6.7, absorption: 0.25,
      sfa: 19, mufa: 25, pufa: 27, epa: 25, heme: true }),
  // Жирная рыба — единственный дешёвый источник ЭПК+ДГК.
  // Сельдь берётся ФИЛЕ: считаем съедобные граммы, а у целой рыбы половина веса —
  // голова, хребет и кожа, и «дешевле за килограмм» там обманчиво.
  herring: F("Сельдь атлантическая (филе)", 695,
    { pack: { brand: "Экстра Фиш", item: "филе сельди слабосолёное", size: "200 г", price: 139 },
      photo: "omega",
      kcal: 158, protein: 18, fat: 9, calcium: 57, iron: 1.1, absorption: 0.15,
      sfa: 23, mufa: 41, pufa: 24, epa: 18 }),
  mackerel: F("Скумбрия", 340,
    { kcal: 191, protein: 18, fat: 13.2, calcium: 12, iron: 1.6, absorption: 0.15,
      sfa: 24, mufa: 40, pufa: 24, epa: 20 }),
  // Молочка — кальций. Дешевле кефира на грамм кальция ничего нет.
  // ЖИР МОЛОЧКИ — в целой глобуле, и в баланс он НЕ идёт (local.rs: глобульный
  // ингредиент исключается из fa_balance). Поэтому насыщенный жир кефира и творога
  // ничего здесь не портит, а обезжиренные версии брать незачем.
  milk: F("Молоко 2.5%", 90,
    { kcal: 54, protein: 2.9, fat: 2.5, calcium: 120, iron: 0.1,
      sfa: 64, mufa: 28, pufa: 4, globule: true }),
  kefir: F("Кефир 2.5%", 113,
    { pack: { brand: "Простоквашино", item: "кефир 2,5 %", size: "930 г", price: 105 },
      photo: "kefir", photoFit: "contain",
      kcal: 53, protein: 3, fat: 2.5, calcium: 120, iron: 0.1,
      sfa: 64, mufa: 28, pufa: 4, globule: true }),
  // Сыр — самый дешёвый кальций на грамм после молока, и при этом ПЛОТНЫЙ по
  // калориям: закрывает и недобор ккал, освободившийся после урезанного кефира.
  cheese: F("Сыр полутвёрдый 45%", 995,
    { pack: { brand: "Брест-Литовск", item: "сыр Финский 45 %", size: "200 г", price: 199 },
      photo: "cheese",
      kcal: 340, protein: 25, fat: 26, calcium: 900, iron: 0.7,
      sfa: 63, mufa: 28, pufa: 3, globule: true }),
  // Греческий йогурт: белок вдвое гуще обычного йогурта, жир — глобульный.
  teos: F("Греческий йогурт Teos 2%", 564,
    { pack: { brand: "Teos", item: "йогурт греческий густой 2 %", size: "140 г", price: 79 },
      photo: "teos", photoFit: "contain",
      kcal: 66, protein: 9, fat: 2, calcium: 120, iron: 0.1,
      sfa: 64, mufa: 28, pufa: 4, globule: true }),
  cottage: F("Творог 5%", 417,
    { pack: { brand: "Константиново", item: "творог 5 %", size: "180 г", price: 75 },
      photo: "protein",
      kcal: 121, protein: 17, fat: 5, calcium: 164, iron: 0.4,
      sfa: 63, mufa: 28, pufa: 4, globule: true }),
  // Цена яйца — за СЪЕДОБНЫЙ килограмм: десяток С1 ≈ 580 г брутто, за вычетом
  // скорлупы ≈ 510 г. Иначе яйцо кажется дешевле, чем оно есть.
  egg: F("Яйцо куриное", 194,
    { pack: { brand: "Ozon fresh", item: "яйцо куриное столовое С1", size: "10 шт", price: 99 },
      photo: "eggs",
      kcal: 157, protein: 12.7, fat: 11.5, calcium: 55, iron: 1.75, absorption: 0.1,
      sfa: 31, mufa: 44, pufa: 15, epa: 1, egg: true }),
  // Овощи и фрукты — самые дешёвые «объёмные».
  cabbage: F("Капуста белокочанная", 81,
    { pack: { brand: null, item: "капуста кочанная, Россия", size: "1 кг", price: 81 },
      photo: "cabbage",
      kcal: 28, protein: 1.8, fat: 0.1, fibre: 2.0, calcium: 48, iron: 0.6, veg: true }),
  carrot: F("Морковь", 65,
    { pack: { brand: null, item: "морковь колхозная", size: "1 кг", price: 65 },
      photo: "carrot", photoFit: "contain",
      kcal: 35, protein: 1.3, fat: 0.1, fibre: 2.4, calcium: 27, iron: 0.7, veg: true }),
  beet: F("Свёкла", 90,
    { pack: { brand: null, item: "свёкла, Россия", size: "1 кг", price: 90 },
      photo: "beet", photoFit: "contain",
      kcal: 42, protein: 1.5, fat: 0.1, fibre: 2.5, calcium: 37, iron: 1.4, veg: true }),
  apple: F("Яблоко", 257,
    { pack: { brand: null, item: "яблоки свежие российские", size: "2 кг", price: 514 },
      photo: "apple", photoFit: "contain",
      kcal: 47, protein: 0.4, fat: 0.4, fibre: 1.8, calcium: 16, iron: 2.2, veg: true }),
  frozenVeg: F("Овощная смесь замороженная", 160,
    { kcal: 45, protein: 2.2, fat: 0.3, fibre: 3.0, calcium: 30, iron: 0.8, veg: true }),
  // Крупы.
  oats: F("Овсяные хлопья", 130,
    { kcal: 352, protein: 12.3, fat: 6.2, fibre: 10.0, calcium: 52, iron: 3.6,
      sfa: 18, mufa: 33, pufa: 38 }),
  barley: F("Перловка", 60,
    { kcal: 315, protein: 9.3, fat: 1.1, fibre: 15.6, calcium: 38, iron: 1.8,
      sfa: 21, mufa: 13, pufa: 48 }),
  buckwheat: F("Гречка", 97,
    { pack: { brand: "Агро-Альянс", item: "гречневая крупа ядрица", size: "900 г", price: 87 },
      photo: "buckwheat",
      kcal: 308, protein: 12.6, fat: 3.3, fibre: 11.3, calcium: 20, iron: 6.7,
      sfa: 21, mufa: 30, pufa: 30 }),
  // Бобовые — клетчатка и железо задёшево.
  lentils: F("Чечевица", 193,
    { pack: { brand: "Увелка", item: "чечевица красная", size: "450 г", price: 87 },
      photo: "lentils",
      kcal: 295, protein: 24, fat: 1.5, fibre: 11.5, calcium: 83, iron: 11.8,
      sfa: 15, mufa: 18, pufa: 46 }),
  // ВКУСНЕНЬКОЕ. Жир у сырка молочный (глобула) — в баланс не идёт; у батончика и
  // шоколада жир кондитерский, и он в балансе участвует наравне с маслом.
  // Сырок дешевле КОРОБКОЙ: поштучно тот же вес обходится вдвое дороже.
  syrok: F("Сырок глазированный", 924,
    { pack: { brand: "Ростагроэкспорт", item: "сырок творожный глазированный с ванилью 20 %", size: "12 × 45 г", price: 499 },
      photo: "syrok",
      kcal: 407, protein: 8.5, fat: 26, calcium: 135, iron: 0.3,
      sfa: 63, mufa: 28, pufa: 4, globule: true }),
  barni: F("Мишка Барни", 700,
    { kcal: 463, protein: 6, fat: 21, fibre: 1.5, calcium: 60, iron: 1.2,
      sfa: 45, mufa: 38, pufa: 13 }),
  chocolate: F("Шоколад молочный", 900,
    { kcal: 558, protein: 7.3, fat: 32, fibre: 3.4, calcium: 189, iron: 2.4,
      sfa: 60, mufa: 33, pufa: 3 }),
  // Жиры.
  sunflowerOil: F("Масло подсолнечное", 154,
    { pack: { brand: "Золотая Семечка", item: "масло подсолнечное рафинированное", size: "1 л", price: 142 },
      photo: null,   // fats.jpg — это рыба; фото бутылки масла пока нет
      kcal: 899, protein: 0, fat: 99.9, sfa: 11, mufa: 24, pufa: 63 }),
  walnuts: F("Грецкий орех (ядро)", 900,
    { kcal: 654, protein: 15.2, fat: 65.2, fibre: 6.7, calcium: 98, iron: 2.9,
      sfa: 10, mufa: 14, pufa: 72 }),
  sunflowerSeeds: F("Семечки подсолнечника", 240,
    { pack: { brand: null, item: "семечки подсолнечника очищенные сырые", size: "500 г", price: 120 },
      photo: null,
      kcal: 601, protein: 20.7, fat: 52.9, fibre: 8.6, calcium: 78, iron: 5.3,
      sfa: 10, mufa: 20, pufa: 66 }),
  // На странице корзины семечки названы ТЫКВЕННЫМИ — считаем их же, а не
  // подсолнечные: у тыквенных вчетверо цена, вдвое железо и другой профиль жира.
  pumpkinSeeds: F("Семечки тыквенные", 988,
    { pack: { brand: "Ореховец", item: "семена тыквы очищенные", size: "500 г", price: 494 },
      photo: "seeds",
      kcal: 559, protein: 30.2, fat: 49.1, fibre: 6.0, calcium: 46, iron: 8.8,
      sfa: 17, mufa: 31, pufa: 46 }),
};

/// Карточка одной строкой: «Бренд, наименование, фасовка — цена ₽».
/// У безымянных позиций (капуста, морковь) бренда нет — начинаем с наименования.
export function packLabel(pack) {
  if (!pack) return null;
  const head = pack.brand ? `${pack.brand}, ${pack.item}` : pack.item;
  return `${head}, ${pack.size} — ${pack.price} ₽`;
}

/// Счёт корзины: граммы за неделю → суммы, строки и цели профиля.
///
/// Возвращает ровно то, по чему судится корзина: `sum` (недельные суммы),
/// `rows` (строки с ценой) и `targets` (пороги под этот профиль).
export function evaluate(basket, { kcal, sex, weight, height, age }) {
  const targets = {
    proteinDay: proteinTarget(kcal, weight, height, age, sex),
    vegDay: sex === "male" ? RULES.vegPerDayM : RULES.vegPerDayF,
    calciumDay: RULES.calciumPerDay,
    fiberDay: Math.max(kcal / 1000 * RULES.fiberPer1000, RULES.fiberMinPerDay),
    ironWeek: ironWeekTarget(sex, age),
  };

  const sum = { kcal: 0, protein: 0, fat: 0, fibre: 0, calcium: 0, ironAbs: 0,
                veg: 0, egg: 0, red: 0, heme: 0, epa: 0, sat: 0, unsat: 0, price: 0 };
  const rows = [];
  for (const [key, grams] of Object.entries(basket)) {
    if (!grams) continue;
    const f = FOODS[key];
    if (!f) throw new Error(`нет такого продукта: ${key}`);
    const k = grams / 100;
    const fat = f.fat * k;
    sum.kcal += f.kcal * k;
    sum.protein += f.protein * k;
    sum.fat += fat;
    sum.fibre += f.fibre * k;
    sum.calcium += f.calcium * k;
    sum.ironAbs += f.iron * k * f.absorption;
    if (f.veg) sum.veg += grams;
    if (f.egg) sum.egg += grams;
    if (f.red) sum.red += grams;
    if (f.heme) sum.heme += f.protein * k;
    sum.epa += fat * f.epa / 100;
    // Баланс считается БЕЗ глобульного жира — как в приложении.
    if (!f.globule) {
      sum.sat += fat * f.sfa / 100;
      sum.unsat += fat * (f.mufa + f.pufa) / 100;
    }
    const cost = f.price * grams / 1000;
    sum.price += cost;
    rows.push({ key, name: f.name, grams, day: Math.round(grams / 7), cost,
                pack: f.pack, photo: f.photo, pricePerKg: f.price });
  }
  return { sum, rows, targets };
}
