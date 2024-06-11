use std::fmt::{Display, Formatter};
use std::ops::{Deref, DerefMut};
use std::str::FromStr;

use itertools::Itertools;
use serde::{Deserialize, Deserializer, de::Error as DeError};
use sqlx::{Database, Decode, FromRow, Postgres, Type, database::HasValueRef, error::BoxDynError, postgres::{PgHasArrayType, PgTypeInfo}};
use split_first_char::SplitFirstChar;
use strum::{Display, EnumString};

#[derive(FromRow, Debug, Clone)]
pub struct Course {
    pub code: String,
    pub name_se: String,
    pub name_en: Option<String>,
    pub url: String,
    pub points: f32,
    pub pace: Option<Pace>,
    pub prerequisites: Option<String>,
    pub register_info: Option<String>,
    pub modules: Option<String>,
    pub period_start: Option<i16>,
    pub period_end: Option<i16>,
    pub site: Option<Site>,
    pub language: Option<Language>,
    pub difficulty: Difficulty,
    pub categories: Categories,
    pub conduct: Option<String>,
}

#[derive(Type, Default, Debug, Clone, Copy, Hash, Eq, PartialEq, Ord, PartialOrd)]
pub struct Percentage(i8);

impl Deref for Percentage {
    type Target = i8;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for Percentage {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl TryFrom<i8> for Percentage {
    type Error = ();

    fn try_from(value: i8) -> Result<Self, Self::Error> {
        if (0..=100).contains(&value) {
            Ok(Self(value))
        } else {
            Err(())
        }
    }
}

impl From<Percentage> for i8 {
    fn from(val: Percentage) -> Self {
        val.0
    }
}

impl Display for Percentage {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}%", self.0)
    }
}

#[derive(Type, Debug, Clone, Copy, Hash, Eq, PartialEq, Display, EnumString)]
pub enum Time {
    #[strum(serialize = "Dagtid", ascii_case_insensitive)]
    Day,
    #[strum(serialize = "Veckoslut", ascii_case_insensitive)]
    Weekend,
    #[strum(serialize = "Blandad undervisningstid", ascii_case_insensitive)]
    Mixed,
}

#[derive(Type, Debug, Clone, Copy, Hash, Eq, PartialEq)]
#[sqlx(type_name = "pace_enum")]
pub struct Pace {
    time: Time,
    percentage: Percentage,
}

impl FromStr for Pace {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        fn parse(s: &str) -> Option<Pace> {
            let (time, percentage) = s.rsplit_once(' ')?;

            let time = time.parse().ok()?;
            let percentage = percentage
                .parse::<i8>()
                .ok()?
                .try_into()
                .ok()?;

            Some(Pace { time, percentage })
        }

        parse(s).ok_or(())
    }
}

impl<'de> Deserialize<'de> for Pace {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error> where D: Deserializer<'de> {
        fn parse<'de>(deserializer: impl Deserializer<'de>) -> Option<Pace> {
            let s = <&str as Deserialize>::deserialize(deserializer).ok()?;
            let (time, percentage) = s.split_first_char()?;

            let time = match time {
                'd' => Time::Day,
                'w' => Time::Weekend,
                'm' => Time::Mixed,
                _ => return None
            };
            let percentage = percentage
                .parse::<i8>()
                .ok()?
                .try_into()
                .ok()?;

            Some(Pace { time, percentage })
        }

        parse(deserializer).ok_or_else(|| DeError::custom("invalid format"))
    }
}

impl PgHasArrayType for Pace {
    fn array_type_info() -> PgTypeInfo {
        PgTypeInfo::with_name("_pace_enum")
    }
}

impl Display for Pace {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} {}", self.time, self.percentage)
    }
}

#[derive(Type, Debug, Clone, Copy, Hash, PartialEq, Eq, Display, EnumString)]
#[sqlx(type_name = "site_enum")]
pub enum Site {
    #[strum(serialize = "Stockholm")]
    Stockholm,
    #[strum(serialize = "Piteå")]
    Pitea,
    #[strum(serialize = "Skellefteå")]
    Skelleftea,
    #[strum(serialize = "Luleå")]
    Lulea,
    #[strum(serialize = "Kiruna")]
    Kiruna,
    #[strum(serialize = "Ortsoberoende")]
    LocationIndependent,
}

impl<'de> Deserialize<'de> for Site {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error> where D: Deserializer<'de> {
        match Deserialize::deserialize(deserializer)? {
            "st" => Ok(Self::Stockholm),
            "pt" => Ok(Self::Pitea),
            "sk" => Ok(Self::Skelleftea),
            "lu" => Ok(Self::Lulea),
            "kr" => Ok(Self::Kiruna),
            "li" => Ok(Self::LocationIndependent),
            other => Err(DeError::unknown_variant(other, &["st", "pt", "sk", "lu", "kr", "li"]))
        }
    }
}

impl PgHasArrayType for Site {
    fn array_type_info() -> PgTypeInfo {
        PgTypeInfo::with_name("_site_enum")
    }
}

#[derive(Type, Debug, Clone, Copy, Hash, PartialEq, Eq, Display, EnumString)]
#[sqlx(type_name = "language_enum")]
pub enum Language {
    #[strum(serialize = "Svenska")]
    Swedish,
    #[strum(serialize = "Engelska")]
    English,
}

impl<'de> Deserialize<'de> for Language {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error> where D: Deserializer<'de> {
        match Deserialize::deserialize(deserializer)? {
            "sv" => Ok(Self::Swedish),
            "en" => Ok(Self::English),
            other => Err(DeError::unknown_variant(other, &["sv", "en"]))
        }
    }
}

impl PgHasArrayType for Language {
    fn array_type_info() -> PgTypeInfo {
        PgTypeInfo::with_name("_language_enum")
    }
}

#[derive(Type, Debug, Clone, Copy, Hash, PartialEq, Eq, Display, EnumString)]
#[sqlx(type_name = "difficulty_enum")]
pub enum Difficulty {
    #[strum(serialize = "Förberedande nivå")]
    Preparatory,
    #[strum(serialize = "Grundnivå")]
    Undergraduate,
    #[strum(serialize = "Avancerad nivå")]
    Advanced,
    #[strum(serialize = "Fortsättningskurs på grundnivå")]
    ContinuationUndergraduate,
    #[strum(serialize = "Fortsättningskurs på avancerad nivå")]
    ContinuationAdvanced,
    #[strum(serialize = "Nybörjarkurs på grundnivå")]
    IntroductoryUndergraduate,
}

impl<'de> Deserialize<'de> for Difficulty {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error> where D: Deserializer<'de> {
        match Deserialize::deserialize(deserializer)? {
            "pr" => Ok(Self::Preparatory),
            "ug" => Ok(Self::Undergraduate),
            "ad" => Ok(Self::Advanced),
            "cu" => Ok(Self::ContinuationUndergraduate),
            "ca" => Ok(Self::ContinuationAdvanced),
            "iu" => Ok(Self::IntroductoryUndergraduate),
            other => Err(DeError::unknown_variant(other, &["pr", "ug", "ad", "cu", "ca", "iu"]))
        }
    }
}

impl PgHasArrayType for Difficulty {
    fn array_type_info() -> PgTypeInfo {
        PgTypeInfo::with_name("_difficulty_enum")
    }
}

#[derive(Type, Debug, Clone, Copy, Hash, PartialEq, Eq, Display, EnumString)]
#[sqlx(type_name = "category_enum")]
pub enum Category {
    #[strum(serialize = "Data och IT")]
    Data,
    #[strum(serialize = "Ekonomi, organisation och företagande")]
    Economy,
    #[strum(serialize = "Energi, miljö och hållbar utveckling")]
    Environment,
    #[strum(serialize = "Hälsa, vård och idrott")]
    Health,
    #[strum(serialize = "Juridik och rättsvetenskap")]
    Law,
    #[strum(serialize = "Lärare, undervisning och pedagogik")]
    Education,
    #[strum(serialize = "Musik och teater")]
    Music,
    #[strum(serialize = "Samhällsvetenskap")]
    Social,
    #[strum(serialize = "Teknik")]
    Technology,
    #[strum(serialize = "Media")]
    Media,
    #[strum(serialize = "Tvärvetenskap")]
    Interdisciplinary,
    #[strum(serialize = "Språk")]
    Language,
    #[strum(serialize = "Matematik och naturvetenskap")]
    Mathematics,
    #[strum(serialize = "Information och kommunikation")]
    Information,
    #[strum(serialize = "Humaniora")]
    Humanities,
    #[strum(serialize = "Beteendevetenskap")]
    Behavioral,
}

impl<'de> Deserialize<'de> for Category {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error> where D: Deserializer<'de> {
        match Deserialize::deserialize(deserializer)? {
            "dat" => Ok(Self::Data),
            "eco" => Ok(Self::Economy),
            "env" => Ok(Self::Environment),
            "hth" => Ok(Self::Health),
            "law" => Ok(Self::Law),
            "edu" => Ok(Self::Education),
            "mus" => Ok(Self::Music),
            "soc" => Ok(Self::Social),
            "tec" => Ok(Self::Technology),
            "med" => Ok(Self::Media),
            "ind" => Ok(Self::Interdisciplinary),
            "lng" => Ok(Self::Language),
            "mat" => Ok(Self::Mathematics),
            "inf" => Ok(Self::Information),
            "hum" => Ok(Self::Humanities),
            "bhv" => Ok(Self::Behavioral),
            other => Err(DeError::unknown_variant(other, &["dat", "eco", "env", "hth", "law", "edu", "mus", "soc", "tec", "med", "ind", "lng", "mat", "inf", "hum", "bhv"]))
        }
    }
}

#[derive(Debug, Default, Clone, Hash, PartialEq, Eq)]
pub struct Categories(Vec<Category>);

impl Deref for Categories {
    type Target = Vec<Category>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

// Quadratic time is fine since the vectors are always small.
impl Categories {
    pub fn matches_any(&self, cats: impl IntoIterator<Item = Category>) -> bool {
        cats
            .into_iter()
            .any(|c| self.iter().contains(&c))
    }

    pub fn matches_all(&self, cats: impl IntoIterator<Item = Category>) -> bool {
        cats
            .into_iter()
            .all(|c| self.iter().contains(&c))
    }
}

impl DerefMut for Categories {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl From<Vec<Category>> for Categories {
    fn from(inner: Vec<Category>) -> Self {
        Self(inner)
    }
}

impl Type<Postgres> for Categories {
    fn type_info() -> <Postgres as Database>::TypeInfo {
        PgTypeInfo::with_name("_category_enum")
    }
}

impl<'r> Decode<'r, Postgres> for Categories {
    fn decode(value: <Postgres as HasValueRef<'r>>::ValueRef) -> Result<Self, BoxDynError> {
        Ok(Self(Vec::decode(value)?))
    }
}
