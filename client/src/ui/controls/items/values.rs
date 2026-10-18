//! Tooltip-only item materialization. Mirrors the server's item_random and
//! player_combat_stats rules: wire enchantment values are quality fractions.
use super::*;

#[derive(Default, Deserialize)]
pub struct Data {
    items: HashMap<String, Item>, enchantments: HashMap<String, Enchantment>,
    progressions: HashMap<u32, HashMap<String, f32>>,
    tiers: HashMap<u32, Vec<[f32;2]>>, base_tiers: HashMap<u32, Vec<[f32;2]>>,
}
#[derive(Deserialize)]
struct Item { base: Vec<Modifier>, unique: Vec<Modifier>, base_enchantments: Vec<String>, enchantments: Vec<String>, scaled: bool }
#[derive(Deserialize)]
struct Enchantment { modifiers: Vec<Modifier>, scaled: bool, tier: bool, min_level: u32, max_level: u32, reference: u32 }
#[derive(Deserialize)]
struct Modifier { name: String, low: f32, high: f32, relative: bool, factor: String, explicit: bool,
    #[serde(default = "item_local_default")] item_local: bool }
fn item_local_default() -> bool { true } // Compatibility with older converted catalogs.
#[derive(Clone, Debug)]
pub(super) struct Value { pub name: String, pub amount: f32, pub relative: bool, pub item_local: bool, pub quality: Option<f32> }
#[derive(Default)]
pub(super) struct Values { pub base: Vec<Value>, pub gems: Vec<Value>, pub enchantments: Vec<Value>, pub unique: Vec<Value> }

impl Data {
    pub fn knows(&self, name:&str)->bool {self.enchantments.get(name).is_some_and(|e|!e.modifiers.is_empty())}
    fn factor(&self, level: u32, name: &str) -> f32 {
        self.progressions.get(&level).or_else(||self.progressions.iter().min_by_key(|(n,_)|(n.abs_diff(level),**n)).map(|(_,r)|r))
            .and_then(|r|r.get(name)).copied().unwrap_or(1.0)
    }
    fn value(&self, m: &Modifier, level: u32, reference: u32, scaled: bool, quality: Option<f32>, multiplier: f32) -> Value {
        let mut amount = quality.map_or(m.high,|q|m.low+(m.high-m.low)*q.clamp(0.0,1.0))*multiplier;
        if (scaled || m.explicit) && (!m.relative || scaled) && !m.factor.is_empty() {
            let reference=self.factor(reference,&m.factor);
            amount*=self.factor(level,&m.factor)/if reference==0.0 {1.0} else {reference};
        }
        Value {name:m.name.clone(),amount,relative:m.relative,item_local:m.item_local,quality}
    }
    fn tier(tiers:&HashMap<u32,Vec<[f32;2]>>, tier:u32, rarity:u8, roll:f32)->f32 {
        tiers.get(&tier).or_else(||tiers.get(&0)).and_then(|r|r.get((rarity as usize).min(r.len().saturating_sub(1))))
            .map_or(1.0,|[low,high]|low+(high-low)*roll.clamp(0.0,1.0))
    }
    pub(super) fn item(&self, item:&ItemInfo)->Values {
        let mut result=Values::default();
        let Some(def)=self.items.get(&item.template) else{return result;};
        let level=item.durability.max(1);
        let mut rolls=Rolls::new(item.eighteenth);
        for m in &def.base {result.base.push(self.value(m,level,1,def.scaled && m.item_local,(m.low!=m.high).then(||rolls.next()),1.0));}
        let mut rolls=Rolls::new((item.eighteenth as f32*0.5) as u32);
        for id in &def.base_enchantments {
            let Some(e)=self.enchantments.get(id) else{continue;};
            for m in &e.modifiers {
                let quality=(m.low!=m.high).then(|| {
                    let roll=rolls.next();
                    if e.tier {Self::tier(&self.base_tiers,item.tier,item.kind,roll)} else {roll}
                });
                result.base.push(self.value(m,level.clamp(e.min_level,e.max_level.max(e.min_level)),e.reference,e.scaled,quality,1.0));
            }
        }
        let mut rolls=Rolls::new(item.eighteenth);
        for m in &def.unique {result.unique.push(self.value(m,level,1,def.scaled && m.item_local,(m.low!=m.high).then(||rolls.next()),1.0));}
        for stat in &item.statistics {
            let Some(e)=self.enchantments.get(&stat.name) else{continue;};
            let multiplier=if e.tier {Self::tier(&self.tiers,stat.third,stat.kind.max(0) as u8,stat.value)} else {1.0};
            for m in &e.modifiers {
                let mut value=self.value(m,level.clamp(e.min_level,e.max_level.max(e.min_level)),e.reference,e.scaled,if e.tier {None} else {Some(stat.value)},multiplier);
                value.quality=Some(stat.value.clamp(0.0,1.0));
                result.enchantments.push(value);
            }
        }
        if item.statistics.is_empty() {
            for id in &def.enchantments {if let Some(e)=self.enchantments.get(id) {for m in &e.modifiers {
                result.enchantments.push(self.value(m,level.clamp(e.min_level,e.max_level.max(e.min_level)),e.reference,e.scaled,None,1.0));
            }}}
        }
        for gem in &item.names {
            let Some(gem)=self.items.get(gem) else{continue;};
            for m in gem.base.iter().chain(&gem.unique) {result.gems.push(self.value(m,level,1,gem.scaled,None,1.0));}
            for id in gem.base_enchantments.iter().chain(&gem.enchantments) {if let Some(e)=self.enchantments.get(id) {for m in &e.modifiers {
                result.gems.push(self.value(m,level.clamp(e.min_level,e.max_level.max(e.min_level)),e.reference,e.scaled,None,1.0));
            }}}
        }
        result
    }
}

impl Values {
    pub fn total(&self, name:&str)->f32 {
        let mut absolute=0.0;let mut relative=0.0;
        for v in self.base.iter().chain(&self.gems).chain(&self.enchantments).chain(&self.unique).filter(|v|v.name==name && v.item_local) {
            if v.relative {relative+=v.amount;} else {absolute+=v.amount;}
        }
        absolute*(1.0+relative)
    }
    pub fn contribution(&self, value:&Value)->Option<f32> {
        if !value.item_local {return None;}
        if !value.relative {return Some(value.amount);}
        let mut base=self.base.iter().chain(&self.gems).chain(&self.unique).filter(|v|v.name==value.name && v.item_local && !v.relative).peekable();
        base.peek()?;
        Some(base.map(|v|v.amount).sum::<f32>()*value.amount)
    }
}

use crate::gameplay::item_random::Rolls;

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_item_rolls_match_server_fixtures() {
        for (seed,expected) in [(8477,[0.14880858,0.7770126,0.51011575,0.89454255,0.30557508,0.3343851]),
            (4238,[0.40949348,0.54600966,0.8602403,0.59504724,0.3777675,0.14710325])] {
            let mut rolls=Rolls::new(seed);
            for expected in expected {assert!((rolls.next()-expected).abs()<0.000001);}
        }
    }
    #[test]
    fn quality_is_materialized_before_progression_and_totals() {
        let data:Data=serde_json::from_value(serde_json::json!({
            "items":{"weapon":{"base":[],"unique":[],"base_enchantments":["base"],"enchantments":[],"scaled":true},
                "ruby":{"base":[{"name":"Damage","low":700,"high":700,"relative":false,"factor":"","explicit":false}],"unique":[],"base_enchantments":[],"enchantments":[],"scaled":false}},
            "enchantments":{"base":{"modifiers":[{"name":"Damage","low":100,"high":200,"relative":false,"factor":"DamageFactorAbsolute","explicit":false}],"scaled":true,"tier":false,"min_level":1,"max_level":200,"reference":1},
                "bonus":{"modifiers":[{"name":"Damage","low":0.1,"high":0.3,"relative":true,"factor":"","explicit":false}],"scaled":false,"tier":false,"min_level":1,"max_level":200,"reference":1}},
            "progressions":{"1":{"DamageFactorAbsolute":1},"100":{"DamageFactorAbsolute":10}},"tiers":{},"base_tiers":{}
        })).unwrap();
        let item=ItemInfo {template:"weapon".into(),durability:100,eighteenth:8477,names:vec!["ruby".into()],
            statistics:vec![dsor_proto::commands::inventory::ItemStatistic {name:"bonus".into(),value:0.5,..default()}],..default()};
        let result=data.item(&item);
        assert!((result.base[0].amount-1409.4935).abs()<0.001);
        assert!((result.enchantments[0].amount-0.2).abs()<0.00001);
        assert!((result.total("Damage")-2531.3922).abs()<0.002);
        assert!((result.contribution(&result.enchantments[0]).unwrap()-421.8987).abs()<0.002);
    }
}
