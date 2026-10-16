//! Item cells share the authored window's hit regions, anchors and visibility.
//! Requests never invent item IDs or amounts: server snapshots drive the view.
use super::*;
use crate::gameplay::inventory::{InventoryState, Place, BAG_SLOTS, OVERFLOW_PAGE_SLOTS};
use dsor_proto::commands::inventory::ItemInfo;

#[derive(Deserialize, Clone)]
pub struct Definition {
    pub name: String, pub description: String, pub category: String, pub icon: String,
    pub max_stack: u32, pub required_level: u32, pub slots: Vec<i8>, pub class_name: String, pub rarity: String,
    #[serde(default)] pub icon_config: String,
    /// The raw _Template_Item ItemCategory ("Mount", "Armor"...).
    #[serde(default)] pub item_category: String,
}
/// A rolled appearance's icon and icon configuration (_Template_ItemAppearance).
#[derive(Deserialize)]
pub struct Appearance { pub icon: String, pub config: String }
/// _Template_IconConfig: the background follows the item's rarity, or is fixed.
#[derive(Deserialize)]
pub struct IconConfig { pub rarity: bool, pub background: String, #[serde(default)] pub frame: String }
#[derive(Deserialize)]
pub struct Ingredient { pub template: String, pub tab: String, pub rarity: usize, pub slot: usize, pub class_name: String, pub unlock: u32 }
#[derive(Asset, TypePath, Deserialize)]
pub struct Catalog { pub items: HashMap<String, Definition>, pub ingredients: Vec<Ingredient>, pub colors: HashMap<String, [f32;4]>,
    #[serde(default)] pub appearances: HashMap<String, Appearance>, #[serde(default)] pub icon_configs: HashMap<String, IconConfig>,
    #[serde(default)] pub rarity_backgrounds: Vec<String> }
impl Catalog {
    /// What a cell shows: the rolled appearance's icon (a generated item's look;
    /// its template's icon is the base model's, the same for every roll), else the
    /// template's; and the background its icon configuration gives it -- the
    /// rarity's (icons_toolkit/bg_rarity_*) when RarityControlled.
    /// SOURCE: tools/inventory_catalog.py (_Template_ItemAppearance, _Template_IconConfig).
    /// FAILURE (2026-10-16): every glove showed warrior_gloves_01a, with no rarity
    ///   background ("les icones des items sont tous les memes").
    pub fn icon(&self, template: &str, item: Option<&ItemInfo>) -> (Option<&str>, Option<&str>) {
        let (icon, background, _) = self.icon_parts(template, item);
        (icon, background)
    }
    /// `icon`, and the frame drawn over it (_Template_IconConfig.FrameTexture:
    /// icons_toolkit/frame_slim_basic, _unique, _set, _mythic).
    /// FAILURE (2026-10-16): without it the icon covered the whole cell, border
    ///   included ("les images sont pas bien dans les slots").
    pub fn icon_parts(&self, template: &str, item: Option<&ItemInfo>) -> (Option<&str>, Option<&str>, Option<&str>) {
        let def = self.items.get(template);
        let look = item.and_then(|i| self.appearances.get(if i.fourth_name.is_empty() { &i.appearance } else { &i.fourth_name }));
        let icon = look.map(|a| a.icon.as_str()).filter(|i| !i.is_empty()).or_else(|| def.map(|d| d.icon.as_str()).filter(|i| !i.is_empty()));
        let config = look.map(|a| a.config.as_str()).filter(|c| !c.is_empty()).or_else(|| def.map(|d| d.icon_config.as_str()));
        let config = config.and_then(|c| self.icon_configs.get(c));
        let background = config.and_then(|c| if c.rarity {
            let rarity = item.map(|i| i.kind as usize).unwrap_or(0);
            self.rarity_backgrounds.get(rarity).map(String::as_str)
        } else { Some(c.background.as_str()) }).filter(|b| !b.is_empty());
        let frame = config.map(|c| c.frame.as_str()).filter(|f| !f.is_empty());
        (icon, background, frame)
    }
}
#[derive(Default, TypePath)]
pub struct CatalogLoader;
impl AssetLoader for CatalogLoader {
    type Asset = Catalog; type Settings = (); type Error = std::io::Error;
    async fn load(&self, r: &mut dyn Reader, _: &(), _: &mut LoadContext<'_>) -> Result<Catalog, Self::Error> {
        let mut b=Vec::new(); r.read_to_end(&mut b).await?; serde_json::from_slice(&b).map_err(std::io::Error::other)
    }
    fn extensions(&self) -> &[&str] { &["items.json"] }
}
#[derive(Resource)]
pub struct Data { pub(in crate::ui) catalog: Handle<Catalog>, tooltip: Handle<Layout> }
pub fn setup(mut commands: Commands, assets: Res<AssetServer>) {
    commands.insert_resource(Data { catalog: assets.load("interface/inventory.items.json"), tooltip: assets.load("interface/tooltip.ui.json") });
}

pub fn sync(net:Option<NonSend<crate::network::net::Net>>,mut windows:Query<&mut WindowControls>) {
    if let Some(net)=net {if net.inventory.ready {for mut c in &mut windows {
        c.bag_pages=(net.inventory.allocation.storage_sizes[2] as usize).div_ceil(BAG_SLOTS as usize).clamp(1,8);
        c.premium_bag=net.inventory.writable(Place::Extra(0));
        c.overflow_pages=net.inventory.allocation.overflow.len().div_ceil(OVERFLOW_PAGE_SLOTS as usize).max(1);
        c.overflow_page=c.overflow_page.min(c.overflow_pages-1);
        if c.bag==EXTRA && !c.premium_bag {c.bag=0;}
        if c.bag<EXTRA {c.bag=c.bag.min(c.bag_pages-1);}
    }}}
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cell { Bag(u32), Equipment(i8), Ingredient(usize) }
pub(super) fn cell(w: &Widget) -> Option<Cell> {
    if w.kind != "Slot" { return None; }
    let leaf=w.id.rsplit('/').next()?;
    if let Some(n)=leaf.strip_prefix("TemplateSlot#") {
        let (r,c)=n.split_once('_')?; let index=r.parse::<u32>().ok()?*7+c.parse::<u32>().ok()?;
        return Some(if w.id.contains("/IngredientBag/") { Cell::Ingredient(index as usize) } else { Cell::Bag(index) });
    }
    let slot=match leaf {
        "Helmet"=>0,"Shoulder"=>1,"Torso"=>2,"Gloves"=>3,"Boots"=>4,"RightHand"=>5,"LeftHand"=>6,
        "Back"=>7,"RingLeftHand"=>8,"RingRightHand"=>9,"Neck"=>10,"Waist"=>11,"WeaponMount"=>12,
        "RuneTrinket1"=>13,"RuneTrinket2"=>14,"RuneTrinket3"=>15,"RuneTrinket4"=>16,"RuneTrinket5"=>17,"RuneTrinket6"=>18,"RuneTrinket7"=>19,
        "JewelTrinket1"=>20,"JewelTrinket2"=>21,"JewelTrinket3"=>22,"Ammo"=>35,"AncientTrinket1"=>24,
        "MercenaryNeck"=>25,"MercenaryBack"=>26,"MercenaryWaist"=>27,"AmmoAlt1"=>35,"AmmoAlt2"=>34,
        _=>return None,
    }; Some(Cell::Equipment(slot))
}

const TABS: [&str;9]=["Basics","Boss","BossEvents","CraftingCores","Ammunition","Event_01","Keys","Cooking","Stain"];
pub const EXTRA: usize = usize::MAX-1;
pub const OVERFLOW: usize = usize::MAX;
fn place(c: Cell, w: &WindowControls) -> Option<Place> {
    Some(match c { Cell::Bag(n)=>match w.bag { EXTRA=>Place::Extra(n), OVERFLOW=>Place::Overflow(w.overflow_page as u32*OVERFLOW_PAGE_SLOTS+n), page=>Place::Bag(page as u32*BAG_SLOTS+n) },
        Cell::Equipment(n)=>Place::Equipment(n), Cell::Ingredient(_)=>return None })
}
fn ingredient<'a>(catalog: &'a Catalog, w: &WindowControls, slot: usize) -> Option<&'a Ingredient> {
    let class=["warrior","mage","ranger","dwarf"].get(w.spells.class as usize).copied().unwrap_or("mage");
    catalog.ingredients.iter().find(|i| i.tab==TABS[w.ingredient.min(8)] && (i.rarity==w.rarity || i.rarity==0)
        && i.slot==slot && (i.class_name=="any" || i.class_name.is_empty() || i.class_name==class))
}

struct DisplayItem<'a> { item: Option<&'a ItemInfo>, template: &'a str, amount: u32, unlock: u32, read_only: bool, waiting: bool }
fn displayed<'a>(cell: Cell, c: &WindowControls, state: &'a InventoryState, catalog: &'a Catalog) -> Option<DisplayItem<'a>> {
    if let Cell::Ingredient(slot)=cell {
        let i=ingredient(catalog,c,slot)?;
        let items:Vec<_>=state.items.values().filter(|x| x.template==i.template && state.allocation.words_180.contains(&x.id)).collect();
        return Some(DisplayItem { template:&i.template, amount:items.iter().map(|x| x.third).sum(), item:items.into_iter().min_by_key(|x|x.id), unlock:i.unlock, read_only:true, waiting:false });
    }
    let item=state.at(place(cell,c)?)?;
    let waiting=matches!(place(cell,c),Some(Place::Overflow(_)));
    Some(DisplayItem { item:Some(item), template:&item.template, amount:item.third, unlock:0, read_only:waiting, waiting })
}

struct DragItem { id:u32, revision:u64, origin:Vec2, active:bool, root:Entity }
struct Split { id:u32, revision:u64, target:Place, total:u32 }
#[derive(Resource,Default)]
pub struct Interaction { drag:Option<DragItem>, split:Option<Split>, pending:Option<(u64,f64)> }
impl Interaction { pub(super) fn cancel_drag(&mut self)->bool { self.drag.take().is_some() } }

fn set_amount(c:&mut WindowControls, amount:u32) {
    let Some(id)=c.buttons.iter().find(|b| matches!(b.action,Action::Edit(_))).map(|b|b.id.clone()) else{return};
    let value=amount.to_string(); c.entries.insert(id,TextEntry { cursor:value.len(),text:value,selected:true });
}
fn amount(c:&WindowControls)->Option<u32> { c.entries.values().next()?.text.trim().parse().ok() }
fn open_split(ui:&mut Interaction, windows:&mut Windows, state:&InventoryState, id:u32) {
    let Some(item)=state.items.get(&id) else{return};
    if item.third<=1 {return;}
    let Some(base)=state.location(id).filter(|p|!matches!(p,Place::Equipment(_))) else{return};
    let Some(target)=state.free_slot(base) else{return};
    for (_,name,mut c,mut v,mut tf) in windows.iter_mut() { if name.0=="unstackwindow" {
        set_amount(&mut c,(item.third/2).max(1)); *v=Visibility::Inherited; tf.translation.z=600.0;
        ui.split=Some(Split{id,revision:state.revision,target,total:item.third});
    }}
}

/// How far an equipment slot's icon_slot_bg frame reaches past the Slot itself,
/// reference pixels (inventory.ui.json: Helmet 949.5..999.7 in 945.9..1003.4).
const EQUIPMENT_FRAME_MARGIN: f32 = 3.6;

/// Two clicks on the same item within this many seconds.
const DOUBLE_CLICK: f64 = 0.4;

/// The move a double click makes: an item in the bags to its equipment slot (a
/// free one of its slots, else the first, swapping), an equipped one to the
/// first free bag cell. None when it cannot be worn (class, level) or go back.
fn equip_toggle(state:&InventoryState, catalog:&Catalog, id:u32, level:u32, class:&str) -> Option<dsor_proto::commands::ClientCommand> {
    let item=state.items.get(&id)?; let meta=catalog.items.get(&item.template)?;
    // A mount is used, not worn: InventoryItemAction 0x13 starts its cast, and
    // again takes it away (DSOR_Server services/inventory_map.py, "use operation").
    if meta.item_category=="Mount" {
        return Some(crate::gameplay::inventory::command(id,dsor_proto::commands::inventory::Inventory::USE,vec![]));
    }
    if let Some(Place::Equipment(_))=state.location(id) {
        let free=state.free_slot(Place::Bag(0))?;
        return state.drop_on(id,free,meta.max_stack);
    }
    if meta.slots.is_empty() || meta.required_level>level || !(meta.class_name.is_empty() || meta.class_name=="any" || meta.class_name==class) {return None;}
    let slot=meta.slots.iter().copied().find(|s|state.at(Place::Equipment(*s)).is_none()).unwrap_or(meta.slots[0]);
    state.drop_on(id,Place::Equipment(slot),meta.max_stack)
}

#[allow(clippy::too_many_arguments)]
pub fn input(mut ui:ResMut<Interaction>,mut pointer:ResMut<Pointer>,mut windows:Windows,
    primary:Query<&Window,With<PrimaryWindow>>,mouse:Res<ButtonInput<MouseButton>>,keys:Res<ButtonInput<KeyCode>>,
    net:Option<NonSendMut<crate::network::net::Net>>,data:Res<Data>,catalogs:Res<Assets<Catalog>>,time:Res<Time<Real>>,
    mut last_click:Local<Option<(u32,f64)>>) {
    let Some(mut net)=net else{return}; let Some(catalog)=catalogs.get(&data.catalog) else{return};
    let Ok(window)=primary.single() else{return};
    let point=window.cursor_position().map(|p|p*(BASE_H/window.height().max(1.0)));
    if !window.focused {ui.drag=None;}
    if ui.pending.is_some_and(|(r,t)|r!=net.inventory.revision || time.elapsed_secs_f64()-t>2.0) {ui.pending=None;}
    let busy=ui.pending.is_some() || net.offline || !net.inventory.ready;
    let dialog=windows.iter().find(|(_,n,_,v,_)|n.0=="unstackwindow" && **v!=Visibility::Hidden).map(|(e,_,_,_,_)|e);
    if let Some(e)=dialog {
        let valid=ui.split.as_ref().is_some_and(|s|s.revision==net.inventory.revision && net.inventory.items.contains_key(&s.id));
        if !valid {windows.get_mut(e).unwrap().3.set_if_neq(Visibility::Hidden); ui.split=None;return;}
        let split=ui.split.as_ref().unwrap();
        let (_,_,mut c,_,tf)=windows.get_mut(e).unwrap();
        let change=pointer.item_command.take();
        if let Some(delta)=change.filter(|d|*d!=0) {let n=(amount(&c).unwrap_or(1) as i64+delta as i64).clamp(1,split.total as i64-1) as u32;set_amount(&mut c,n);}
        if mouse.pressed(MouseButton::Left) {
            if let (Some(b),Some(p))=(c.hovered.map(|i|&c.buttons[i]),point) {if b.action==Action::SplitSlider {
                let x=p.x-window_offset(&tf).x-b.k*(BASE_H*window.width()/window.height().max(1.0)-BASE_W);
                let f=((x-b.rect[0])/(b.rect[2]-b.rect[0])).clamp(0.0,1.0);
                let n=1+(f*(split.total-2) as f32).round() as u32; set_amount(&mut c,n);
            }}
        }
        if change==Some(0) || keys.just_pressed(KeyCode::Enter) {
            if let Some(command)=amount(&c).and_then(|n|net.inventory.split(split.id,n,split.target)) {
                net.send(&command);ui.pending=Some((net.inventory.revision,time.elapsed_secs_f64()));ui.split=None;
                windows.get_mut(e).unwrap().3.set_if_neq(Visibility::Hidden);pointer.focus=None;
            }
        }
        return;
    }
    ui.split=None;pointer.item_command=None;
    let hovered=windows.iter().filter(|(_,n,_,v,_)|n.0=="inventory" && **v!=Visibility::Hidden)
        .find_map(|(e,_,c,_,_)|c.hovered.and_then(|b|match c.buttons[b].action {Action::Item(cell)=>Some((e,cell)),_=>None}));
    // A right click uses the item under the cursor: a mount is called (or sent
    // away), equipment is put on or taken off -- as a double click does.
    if mouse.just_pressed(MouseButton::Right) && !busy {
        if let Some((root,cell))=hovered {
            let (_,_,c,_,_)=windows.get(root).unwrap();
            let class=["warrior","mage","ranger","dwarf"][c.spells.class.min(3) as usize];
            if let Some(id)=place(cell,c).and_then(|p|net.inventory.at(p)).map(|i|i.id) {
                if let Some(cmd)=equip_toggle(&net.inventory,catalog,id,net.level.unwrap_or(1),class) {
                    net.send(&cmd);ui.pending=Some((net.inventory.revision,time.elapsed_secs_f64()));
                }
            }
        }
    }
    if mouse.just_pressed(MouseButton::Left) {
        ui.drag=None;
        if let (false,Some((root,cell)),Some(p))=(busy,hovered,point) {
            let (_,_,c,_,_)=windows.get(root).unwrap();
            if let Some(id)=place(cell,c).and_then(|p|net.inventory.at(p)).map(|i|i.id).filter(|id|net.inventory.movable(*id)) {
                let now=time.elapsed_secs_f64();
                let double=last_click.take().is_some_and(|(last,at)|last==id && now-at<=DOUBLE_CLICK);
                if keys.any_pressed([KeyCode::ControlLeft,KeyCode::ControlRight]) {open_split(&mut ui,&mut windows,&net.inventory,id);pointer.down=None;}
                else if double {
                    // A double click equips a piece from the bags, or takes an
                    // equipped one off into the first free bag cell.
                    let class=["warrior","mage","ranger","dwarf"][c.spells.class.min(3) as usize];
                    if let Some(cmd)=equip_toggle(&net.inventory,catalog,id,net.level.unwrap_or(1),class) {
                        net.send(&cmd);ui.pending=Some((net.inventory.revision,now));
                    }
                    pointer.down=None;
                }
                else {ui.drag=Some(DragItem{id,revision:net.inventory.revision,origin:p,active:false,root});*last_click=Some((id,now));}
            }
        }
    }
    if ui.drag.as_ref().is_some_and(|d|d.revision!=net.inventory.revision || !windows.get(d.root).is_ok_and(|(_,_,_,v,_)|*v!=Visibility::Hidden)) {ui.drag=None;}
    if let Some(d)=ui.drag.as_mut() {if point.is_some_and(|p|p.distance(d.origin)>=4.0) {d.active=true;}}
    if mouse.just_released(MouseButton::Left) {
        if let Some(d)=ui.drag.take().filter(|d|d.active && !busy) {
            if let Some((root,cell))=hovered {
                let (_,_,c,_,_)=windows.get(root).unwrap();
                if let Some(target)=place(cell,c) {
                    let source=net.inventory.items.get(&d.id);
                    let meta=source.and_then(|i|catalog.items.get(&i.template));
                    let valid=if let Place::Equipment(slot)=target {meta.is_some_and(|m|m.slots.contains(&slot)
                        && m.required_level <= net.level.unwrap_or(1)
                        && (m.class_name.is_empty() || m.class_name=="any" || m.class_name==["warrior","mage","ranger","dwarf"][c.spells.class.min(3) as usize]))} else{true};
                    if let Some(cmd)=valid.then(||net.inventory.drop_on(d.id,target,meta.map_or(1,|m|m.max_stack))).flatten() {
                        net.send(&cmd);ui.pending=Some((net.inventory.revision,time.elapsed_secs_f64()));
                    }
                }
            }
        }
    }
}

#[derive(Component)] pub struct Contents;
#[derive(Component)] pub struct Overlay;
fn text(commands:&mut Commands,assets:&AssetServer,screen:&Screen,parent:Entity,value:String,at:Vec3,size:f32,color:Color,width:f32) {
    commands.spawn((Text2d::new(value),TextFont{font:assets.load("fonts/tahoma.ttf").into(),font_size:bevy::text::FontSize::Px(size*TEXT_SCALE*screen.scale),..default()},
        TextColor(color),Anchor::TOP_LEFT,bevy::text::TextBounds::new_horizontal(width*screen.scale),
        Transform::from_translation(at).with_scale(Vec3::splat(1.0/screen.scale)),RenderLayers::layer(LAYER),ChildOf(parent)));
}
fn color(catalog:&Catalog,item:Option<&ItemInfo>)->Color {
    let rgba=item.and_then(|i|catalog.colors.get(&i.twenty_eighth.to_string())).copied().unwrap_or([0.85,0.8,0.67,1.0]);
    Color::srgba(rgba[0],rgba[1],rgba[2],rgba[3])
}

#[allow(clippy::too_many_arguments,clippy::type_complexity)]
pub fn render(mut commands:Commands,windows:Query<(Entity,&Window2018,&WindowControls,&Visibility)>,net:Option<NonSend<crate::network::net::Net>>,
    data:Res<Data>,catalogs:Res<Assets<Catalog>>,assets:Res<AssetServer>,screen:Res<Screen>,old:Query<Entity,With<Contents>>,
    mut last:Local<Option<(Entity,u64,usize,u8,usize,usize,u32,u32,u8,usize)>>) {
    let Some(catalog)=catalogs.get(&data.catalog) else{return}; let Some(net)=net else{return};
    let window=windows.iter().find(|(_,n,_,v)|n.0=="inventory" && **v!=Visibility::Hidden);
    let key=window.map(|(e,_,c,_)|(e,net.inventory.revision,c.bag,c.inventory,c.ingredient,c.rarity,screen.width.to_bits(),screen.scale.to_bits(),c.spells.class,c.overflow_page));
    if *last==key {return;} *last=key; for e in &old {commands.entity(e).despawn();}
    let Some((root,_,c,_))=window else{return};
    let group=commands.spawn((Contents,Transform::from_xyz(0.0,0.0,WINDOW_ITEMS_DEPTH),Visibility::Inherited,ChildOf(root))).id();
    for b in &c.buttons {
        let Action::Item(cell)=b.action else{continue}; if !c.section_visible(b.section) {continue;}
        // An equipment Slot (50 px) sits inside its icon_slot_bg frame (57.5 px); a
        // bag cell is its frame. Drawn over the frame, an equipped icon fills its
        // slot as a bag one does.
        // FAILURE (2026-10-16): drawn in the Slot, equipped icons stood 7 px inside
        //   their frames ("ceux equipes prennent pas la taille entiere du slot").
        let grow=if matches!(cell,Cell::Equipment(_)) {EQUIPMENT_FRAME_MARGIN} else {0.0};
        let [l,t,r,bt]=[b.rect[0]-grow,b.rect[1]-grow,b.rect[2]+grow,b.rect[3]+grow];let pos=screen.world(b.k,l,t);let size=Vec2::new(r-l,bt-t);
        let locked=place(cell,c).is_some_and(|p|!matches!(p,Place::Equipment(_)) && p.cell()>=net.inventory.capacity(p));
        if locked {commands.spawn((Sprite::from_color(Color::srgba(0.01,0.01,0.01,0.8),size),Anchor::TOP_LEFT,Transform::from_translation(pos.extend(0.0)),RenderLayers::layer(LAYER),ChildOf(group)));continue;}
        let Some(d)=displayed(cell,c,&net.inventory,catalog) else{continue};
        let tint=if d.amount==0 {Color::srgba(0.38,0.38,0.38,0.65)} else{Color::WHITE};
        let (icon,background,frame)=catalog.icon_parts(d.template,d.item);
        if let Some(bg)=background {
            commands.spawn((Sprite{image:assets.load(bg.to_owned()),custom_size:Some(size-Vec2::splat(5.0)),color:tint,..default()},Anchor::TOP_LEFT,
                Transform::from_xyz(pos.x+2.5,pos.y-2.5,0.05),RenderLayers::layer(LAYER),ChildOf(group)));
        }
        if let Some(f)=frame {
            commands.spawn((Sprite{image:assets.load(f.to_owned()),custom_size:Some(size-Vec2::splat(5.0)),color:tint,..default()},Anchor::TOP_LEFT,
                Transform::from_xyz(pos.x+2.5,pos.y-2.5,0.15),RenderLayers::layer(LAYER),ChildOf(group)));
        }
        if let Some(icon)=icon {
            commands.spawn((Sprite{image:assets.load(icon.to_owned()),custom_size:Some(size-Vec2::splat(5.0)),color:tint,..default()},Anchor::TOP_LEFT,
                Transform::from_xyz(pos.x+2.5,pos.y-2.5,0.1),RenderLayers::layer(LAYER),ChildOf(group)));
        } else {text(&mut commands,&assets,&screen,group,"?".into(),pos.extend(0.2)+Vec3::new(15.0,-10.0,0.0),22.0,Color::WHITE,size.x);}
        if d.amount>1 || matches!(cell,Cell::Ingredient(_)) {
            let at=Vec3::new(pos.x+3.0,pos.y-size.y+17.0,0.3);
            text(&mut commands,&assets,&screen,group,d.amount.to_string(),at+Vec3::new(1.0,-1.0,-0.01),13.0,Color::BLACK,size.x-4.0);
            text(&mut commands,&assets,&screen,group,d.amount.to_string(),at,13.0,Color::WHITE,size.x-4.0);
        }
    }
}

fn description(d:&DisplayItem,catalog:&Catalog)->(String,String) {
    let meta=catalog.items.get(d.template);
    let title=meta.map_or(d.template,|m|m.name.as_str()).to_owned();
    let mut lines=Vec::new();
    if let Some(m)=meta {if !m.category.is_empty(){lines.push(m.category.clone());} if d.item.is_none(){lines.push(m.rarity.clone());}}
    if let Some(i)=d.item {
        // release261 wire: +0x0c item level is the first word after dye;
        // older field names call it durability. Do not display sale value as level.
        lines.push(format!("Level {}",i.durability));
        if i.flag_0 {lines.push("Unidentified".into());}
        else {for s in &i.statistics {lines.push(format!("{} : {:.2}",s.name,s.value));}}
        for gem in &i.names {lines.push(format!("◆ {}",catalog.items.get(gem).map_or(gem.as_str(),|m|m.name.as_str())));}
    }
    if let Some(m)=meta {
        let required=m.required_level.max(d.unlock);if required>1 {lines.push(format!("Required level: {required}"));}
        if !m.description.is_empty() {lines.push(m.description.replace("\\n","\n"));}
        if m.max_stack>1 {lines.push(format!("Quantity: {} / {}",d.amount,m.max_stack));}
    }
    if d.item.is_some() && d.amount>1 && !d.read_only {lines.push("Ctrl + click: split stack".into());}
    if d.waiting {lines.push("Free up a slot to collect this item automatically.".into());}
    if d.amount==0 {lines.push("Not yet collected".into());}
    (title,lines.join("\n"))
}

/// Original tooltip corner/edge meshes keep their border thickness as content grows.
fn frame(commands:&mut Commands,assets:&AssetServer,meshes:&mut Assets<Mesh>,materials:&mut Assets<UiMaterial>,root:Entity,layout:&Layout,w:f32,h:f32) {
    for widget in &layout.widgets {
        if !widget.id.starts_with("Canvas/BGLabel/") {continue;}
        let leaf=widget.id.rsplit('/').next().unwrap_or("");
        if !leaf.ends_with("edge") && !leaf.ends_with("corner") {continue;}
        let Some(mut art)=widget.art.clone() else{continue};
        for p in art.pos.chunks_exact_mut(2) {
            let x=p[0]-416.62;let y=p[1]-413.3;
            p[0]=if x<29.02{x}else if x>162.24{w-(191.0-x)}else{29.02+(x-29.02)*(w-57.78)/133.22};
            p[1]=-(if y<15.4{y}else if y>112.32{h-(127.73-y)}else{15.4+(y-15.4)*(h-30.81)/96.92});
        }
        let atlas=art.tex.as_ref().map(|t|assets.load(t.clone()));let mut run=Run::default();run.push(&art,0.0);
        commands.spawn((Mesh2d(meshes.add(run.mesh())),MeshMaterial2d(materials.add(material(atlas))),Transform::from_xyz(0.0,0.0,0.1),RenderLayers::layer(LAYER),ChildOf(root)));
    }
}

#[allow(clippy::too_many_arguments)]
pub fn overlay(mut commands:Commands,ui:Res<Interaction>,windows:Query<(&Window2018,&WindowControls,&Visibility)>,primary:Query<&Window,With<PrimaryWindow>>,
    net:Option<NonSend<crate::network::net::Net>>,data:Res<Data>,catalogs:Res<Assets<Catalog>>,layouts:Res<Assets<Layout>>,assets:Res<AssetServer>,screen:Res<Screen>,
    old:Query<Entity,With<Overlay>>,mut transforms:Query<&mut Transform,With<Overlay>>,mut meshes:ResMut<Assets<Mesh>>,mut materials:ResMut<Assets<UiMaterial>>,
    mut last:Local<Option<(String,u64,bool,u32)>>) {
    let Some(net)=net else{return};let Some(catalog)=catalogs.get(&data.catalog) else{return};
    let point=primary.single().ok().filter(|w|w.focused).and_then(|w|w.cursor_position().map(|p|p*(BASE_H/w.height().max(1.0))));
    let dragged=ui.drag.as_ref().filter(|d|d.active).and_then(|d|net.inventory.items.get(&d.id));
    let display=dragged.map(|i|DisplayItem{item:Some(i),template:&i.template,amount:i.third,unlock:0,read_only:false,waiting:false}).or_else(||windows.iter().filter(|(n,_,v)|n.0=="inventory" && **v!=Visibility::Hidden)
        .find_map(|(_,c,_)|c.hovered.and_then(|n|match c.buttons[n].action {Action::Item(cell)=>displayed(cell,c,&net.inventory,catalog),_=>None})));
    let key=display.as_ref().filter(|_|point.is_some() && ui.split.is_none()).map(|d|(format!("{}:{}:{}",d.template,d.item.map_or(0,|i|i.id),d.amount),net.inventory.revision,dragged.is_some(),screen.scale.to_bits()));
    let (title,body)=display.as_ref().map(|d|description(d,catalog)).unwrap_or_default();
    let h=(60.0+body.lines().map(|l|1+l.chars().count()/42).sum::<usize>() as f32*17.0).min(BASE_H-20.0);
    let at=point.map(|p|if dragged.is_some(){p-Vec2::splat(22.0)}else{Vec2::new((p.x+18.0).min(screen.width-330.0).max(0.0),(p.y+18.0).min(BASE_H-h).max(0.0))});
    if *last==key {if let Some(p)=at {for mut tf in &mut transforms {tf.translation=screen.world(0.0,p.x,p.y).extend(800.0);}}return;}
    *last=key.clone();for e in &old {commands.entity(e).despawn();}if key.is_none(){return;}
    let (Some(d),Some(p))=(display,at) else{return};
    let root=commands.spawn((Overlay,Transform::from_translation(screen.world(0.0,p.x,p.y).extend(800.0)),Visibility::Inherited)).id();
    if dragged.is_some() {
        let (icon,background)=catalog.icon(d.template,d.item);
        if let Some(bg)=background {commands.spawn((Sprite{image:assets.load(bg.to_owned()),custom_size:Some(Vec2::splat(44.0)),..default()},Anchor::TOP_LEFT,Transform::from_xyz(0.0,0.0,-0.01),RenderLayers::layer(LAYER),ChildOf(root)));}
        if let Some(icon)=icon {commands.spawn((Sprite{image:assets.load(icon.to_owned()),custom_size:Some(Vec2::splat(44.0)),..default()},Anchor::TOP_LEFT,RenderLayers::layer(LAYER),ChildOf(root)));}
        return;
    }
    commands.spawn((Sprite::from_color(Color::srgba(0.025,0.02,0.015,0.97),Vec2::new(330.0,h)),Anchor::TOP_LEFT,RenderLayers::layer(LAYER),ChildOf(root)));
    if let Some(layout)=layouts.get(&data.tooltip) {frame(&mut commands,&assets,&mut meshes,&mut materials,root,layout,330.0,h);}
    text(&mut commands,&assets,&screen,root,title,Vec3::new(18.0,-14.0,0.2),17.0,color(catalog,d.item),294.0);
    text(&mut commands,&assets,&screen,root,body,Vec3::new(18.0,-43.0,0.2),13.0,Color::srgb(0.92,0.87,0.76),294.0);
}

#[cfg(test)]
mod tests {
    use super::*;
    use dsor_proto::commands::inventory::InventoryInfo;

    #[test]
    #[ignore = "requires locally converted game UI assets"]
    fn rendered_windows_keep_items_and_labels_in_order_and_start_with_zero_wisdom() {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, bevy::asset::AssetPlugin::default(), TransformPlugin))
            .init_asset::<Layout>().init_asset::<Mesh>().init_asset::<UiMaterial>()
            .init_asset::<Image>().init_asset::<Font>().init_asset::<Catalog>()
            .init_resource::<Loaded>().init_resource::<Screen>().init_resource::<Figures>()
            .init_resource::<crate::gameplay::stats::Sheet>().init_resource::<crate::actors::monsters::HoverInfo>()
            .init_resource::<Pointer>().init_resource::<Interaction>().init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<bevy::sprite_render::EntitiesNeedingSpecialization<UiMaterial>>()
            .add_systems(PostUpdate, (refresh_mesh_order, bevy::sprite_render::check_entities_needing_specialization::<UiMaterial>)
                .chain().after(TransformSystems::Propagate).after(bevy::asset::AssetEventSystems))
            .add_systems(Update, (spawn_windows, sync, keyboard, order_windows, read_figures, update_fills, update_values, paint, render).chain());
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../assets/interface");
        for (name, z) in HUD_WINDOWS.into_iter().chain(MENU_WINDOWS.map(|(name, _, z)| (name, z))) {
            let layout: Layout = serde_json::from_slice(&std::fs::read(dir.join(format!("{name}.ui.json"))).unwrap()).unwrap();
            let handle = app.world_mut().resource_mut::<Assets<Layout>>().add(layout);
            let mut loaded = app.world_mut().resource_mut::<Loaded>();
            loaded.layouts.push((handle, z, None));
            loaded.spawned.push(false);
        }
        let catalog: Catalog = serde_json::from_slice(&std::fs::read(dir.join("inventory.items.json")).unwrap()).unwrap();
        let template = catalog.ingredients[0].template.clone();
        let catalog = app.world_mut().resource_mut::<Assets<Catalog>>().add(catalog);
        app.insert_resource(Data { catalog, tooltip: default() });
        crate::network::net::start_offline(app.world_mut(), 1, 0);
        app.world_mut().non_send_mut::<crate::network::net::Net>().inventory.apply(InventoryInfo {
            items: vec![ItemInfo { id: 1, third: 23, template, ..default() }],
            placements: vec![(1, 0)], storage_sizes: [0, 0, 224, 0, 0, 0], ..default()
        });
        app.update();
        let roots: HashMap<_, _> = app.world_mut().query::<(Entity, &Window2018)>().iter(app.world())
            .map(|(e, w)| (w.0.clone(), e)).collect();
        let inventory = roots["inventory"];
        let skills = roots["skilltalentswindow"];
        let press = |app: &mut App, key| {
            app.world_mut().resource_mut::<ButtonInput<KeyCode>>().press(key);
            app.update();
            app.world_mut().resource_mut::<ButtonInput<KeyCode>>().reset(key);
        };
        press(&mut app, KeyCode::KeyK);
        assert_eq!(*app.world().get::<Visibility>(skills).unwrap(), Visibility::Hidden);
        press(&mut app, KeyCode::KeyI);
        press(&mut app, KeyCode::KeyS);
        assert_eq!(*app.world().get::<Visibility>(skills).unwrap(), Visibility::Inherited);

        // Measure the actual rendered descendants, including runtime item icons
        // and stack labels, rather than just checking the two root transforms.
        fn depths(world: &World, root: Entity) -> (f32, f32) {
            let mut bounds = (f32::INFINITY, f32::NEG_INFINITY);
            let mut queue = vec![root];
            while let Some(e) = queue.pop() {
                if let Some(children) = world.get::<Children>(e) { queue.extend(children.iter()); }
                if world.get::<Mesh2d>(e).is_some() || world.get::<Sprite>(e).is_some() || world.get::<Text2d>(e).is_some() {
                    let z = world.get::<GlobalTransform>(e).unwrap().translation().z;
                    bounds = (bounds.0.min(z), bounds.1.max(z));
                }
            }
            bounds
        }
        assert!(app.world_mut().query::<&Sprite>().iter(app.world()).count() > 0);
        assert!(depths(app.world(), inventory).1 < depths(app.world(), skills).0);
        for _ in 0..20 {
            // External visibility changes must raise the whole window too.
            *app.world_mut().get_mut::<Visibility>(inventory).unwrap() = Visibility::Hidden;
            app.update();
            *app.world_mut().get_mut::<Visibility>(inventory).unwrap() = Visibility::Inherited;
            app.update();
            assert!(depths(app.world(), skills).1 < depths(app.world(), inventory).0);
            press(&mut app, KeyCode::KeyS);
            press(&mut app, KeyCode::KeyS);
            assert!(depths(app.world(), inventory).1 < depths(app.world(), skills).0);
            assert!(depths(app.world(), skills).1 < 500.0, "focus changes must stay below modal windows");
            let queued = &app.world().resource::<bevy::sprite_render::EntitiesNeedingSpecialization<UiMaterial>>().changed;
            let meshes: Vec<_> = app.world().get::<Children>(inventory).unwrap().iter()
                .chain(app.world().get::<Children>(skills).unwrap().iter())
                .flat_map(|parent| app.world().get::<Children>(parent).into_iter().flat_map(|c| c.iter()))
                .filter(|e| app.world().get::<Mesh2d>(*e).is_some()).collect();
            assert!(!meshes.is_empty());
            assert!(meshes.iter().all(|e| queued.contains(e)), "the renderer must refresh retained mesh sort keys after focus changes");
        }
        app.update();
        assert!(app.world().resource::<bevy::sprite_render::EntitiesNeedingSpecialization<UiMaterial>>().changed.is_empty(),
            "stationary UI must keep its cached render data");
        app.world_mut().resource_mut::<Pointer>().focus = Some((skills, "search".into()));
        press(&mut app, KeyCode::KeyS);
        assert_eq!(*app.world().get::<Visibility>(skills).unwrap(), Visibility::Inherited, "typing must consume shortcuts");

        let mut values = app.world_mut().query::<(&Value, &Text2d)>();
        for (kind, expected, count) in [(ValueKind::WisdomLevel, "0", 2), (ValueKind::WisdomProgress, "0%", 1), (ValueKind::WisdomPoints, "Available Points: 0", 1)] {
            let actual: Vec<_> = values.iter(app.world()).filter(|(v, _)| v.0 == kind).map(|(_, t)| t.0.as_str()).collect();
            assert_eq!(actual, vec![expected; count]);
        }
        let mut gauges = app.world_mut().query::<(&Fill, &MeshMaterial2d<UiMaterial>)>();
        let wisdom: Vec<_> = gauges.iter(app.world()).filter(|(f, _)| f.which == Gauge::Wisdom).collect();
        assert!(wisdom.len() >= 2, "both the HUD and talents wisdom bars must be dynamic");
        for (_, handle) in wisdom {
            let clip = app.world().resource::<Assets<UiMaterial>>().get(&handle.0).unwrap().clip;
            assert_eq!(clip.x, clip.z, "zero wisdom must have an empty progress bar");
        }
        // Late-arriving layouts must receive defaults without a stats update.
        let late = app.world_mut().spawn((Value(ValueKind::WisdomPoints), Text2d::new("Available Points: 19"))).id();
        app.update();
        assert_eq!(app.world().get::<Text2d>(late).unwrap().0, "Available Points: 0");
    }

    #[test]
    #[ignore = "requires locally converted game UI assets"]
    fn inventory_pointer_split_cancellation_and_collection_placeholders() {
        let (mut app, primary, roots, layouts) = super::super::tests::exported_app();
        app.init_resource::<Time<Real>>()
            .init_resource::<Assets<Catalog>>()
            .add_systems(Update, (sync, input).chain().after(edit_text).before(paint));
        crate::network::net::start_offline(app.world_mut(), 1, 0);
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../assets/interface/inventory.items.json");
        let catalog: Catalog = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
        let ingredient_template = catalog.ingredients.iter().find(|i| i.tab == "Basics" && i.rarity == 1 && i.slot == 0).unwrap().template.clone();
        let handle = app.world_mut().resource_mut::<Assets<Catalog>>().add(catalog);
        app.insert_resource(Data { catalog: handle.clone(), tooltip: default() });
        let mut net = app.world_mut().non_send_mut::<crate::network::net::Net>();
        net.offline = false;
        net.inventory.apply(InventoryInfo {
            items: vec![ItemInfo { id: 1, third: 10, template: ingredient_template.clone(), ..default() },
                ItemInfo { id: 2, third: 23, template: ingredient_template, ..default() }],
            placements: vec![(1, 0)], words_180: vec![2], storage_sizes: [0, 0, 224, 0, 0, 0], ..default()
        });
        let inventory = roots["inventory"];
        let dialog = roots["unstackwindow"];
        *app.world_mut().get_mut::<Visibility>(inventory).unwrap() = Visibility::Inherited;
        app.update();
        assert_eq!(app.world().get::<WindowControls>(inventory).unwrap().bag_pages, 8);
        let cursor = |app: &mut App, cell| {
            let c = app.world().get::<WindowControls>(inventory).unwrap();
            let b = c.buttons.iter().find(|b| b.action == Action::Item(Cell::Bag(cell))).unwrap();
            let p = Vec2::new((b.rect[0] + b.rect[2]) * 0.5, (b.rect[1] + b.rect[3]) * 0.5);
            app.world_mut().get_mut::<Window>(primary).unwrap().set_cursor_position(Some(p));
        };
        cursor(&mut app, 0);
        app.world_mut().resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::ControlLeft);
        app.world_mut().resource_mut::<ButtonInput<MouseButton>>().press(MouseButton::Left);
        app.update();
        assert_eq!(*app.world().get::<Visibility>(dialog).unwrap(), Visibility::Inherited);
        assert_eq!(amount(app.world().get::<WindowControls>(dialog).unwrap()), Some(5));
        assert_eq!(app.world().resource::<Interaction>().split.as_ref().unwrap().target, Place::Bag(1));
        app.world_mut().resource_mut::<ButtonInput<MouseButton>>().reset_all();
        app.world_mut().resource_mut::<ButtonInput<KeyCode>>().reset_all();
        app.world_mut().resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::Enter);
        app.update();
        assert_eq!(*app.world().get::<Visibility>(dialog).unwrap(), Visibility::Hidden);
        assert!(app.world().resource::<Interaction>().pending.is_some());
        assert_eq!(app.world().non_send::<crate::network::net::Net>().inventory.at(Place::Bag(0)).unwrap().third, 10,
            "wait for authoritative IDs and amounts, never fabricate split stacks");
        app.world_mut().resource_mut::<ButtonInput<KeyCode>>().reset_all();
        app.world_mut().non_send_mut::<crate::network::net::Net>().inventory.revision += 1;
        cursor(&mut app, 0);
        app.world_mut().resource_mut::<ButtonInput<MouseButton>>().press(MouseButton::Left);
        app.update();
        app.world_mut().resource_mut::<ButtonInput<MouseButton>>().clear();
        cursor(&mut app, 2);
        app.update();
        assert!(app.world().resource::<Interaction>().drag.as_ref().is_some_and(|d| d.active));
        app.world_mut().resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::Escape);
        app.update();
        assert!(app.world().resource::<Interaction>().drag.is_none());
        assert_eq!(*app.world().get::<Visibility>(inventory).unwrap(), Visibility::Inherited);
        let c = app.world().get::<WindowControls>(inventory).unwrap();
        let catalog = app.world().resource::<Assets<Catalog>>().get(&handle).unwrap();
        let state = &app.world().non_send::<crate::network::net::Net>().inventory;
        assert_eq!(displayed(Cell::Ingredient(0), c, state, catalog).unwrap().amount, 23);
        let empty = displayed(Cell::Ingredient(1), c, state, catalog).unwrap();
        assert_eq!(empty.amount, 0);
        assert!(empty.item.is_none());
        assert!(!description(&empty, catalog).0.is_empty());

        let layout = &layouts["inventory"];
        assert_eq!(layout.widgets.iter().filter(|w| matches!(cell(w), Some(Cell::Bag(_)))).count(), 28);
        let background = layout.widgets.iter().position(|w| w.id.ends_with("/IngredientBag/Label_bg/Artwork")).unwrap();
        for tier in 1..=5 {
            let n = layout.widgets.iter().position(|w| w.id.ends_with(&format!("/Button_Rarity{tier}/normal"))).unwrap();
            assert!(background < n, "ingredient background must be painted before tier selectors");
            assert!(!layout.widgets[n].hidden && layout.widgets[n].art.is_some());
        }
        assert!(layout.widgets.iter().filter(|w| w.id.contains("/icon_area_cores/") || w.id.ends_with("/RecipesInfotextArea")).all(|w|w.hidden));
        app.world_mut().resource_mut::<ButtonInput<KeyCode>>().reset_all();
        app.world_mut().resource_mut::<ButtonInput<MouseButton>>().reset_all();
        {
            let mut c=app.world_mut().get_mut::<WindowControls>(inventory).unwrap();
            c.bag=7;
            assert_eq!(place(Cell::Bag(27),&c),Some(Place::Bag(223)));
            assert!(c.section_visible(Section::BagFourthRow));
            assert!(!c.section_visible(Section::OverflowFooter));
            assert!(!c.enabled(c.buttons.iter().find(|b|b.action==Action::Bag(EXTRA)).unwrap()));
        }
        {
            let mut net=app.world_mut().non_send_mut::<crate::network::net::Net>();
            net.inventory.membership(2,true);
            net.inventory.allocation.storage_sizes[5]=28;
        }
        app.update();
        {
            let mut c=app.world_mut().get_mut::<WindowControls>(inventory).unwrap();
            assert!(c.enabled(c.buttons.iter().find(|b|b.action==Action::Bag(EXTRA)).unwrap()));
            c.bag=EXTRA;
            assert_eq!(place(Cell::Bag(27),&c),Some(Place::Extra(27)));
        }
        app.world_mut().non_send_mut::<crate::network::net::Net>().inventory.membership(2,false);
        app.update();
        assert_eq!(app.world().get::<WindowControls>(inventory).unwrap().bag,0);
        {
            let mut net=app.world_mut().non_send_mut::<crate::network::net::Net>();
            net.inventory.allocation.placements.clear();
            net.inventory.allocation.overflow=(1..=28).collect();
            net.inventory.revision+=1;
        }
        app.world_mut().get_mut::<WindowControls>(inventory).unwrap().bag=OVERFLOW;
        cursor(&mut app,0);
        app.world_mut().resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::ControlLeft);
        app.world_mut().resource_mut::<ButtonInput<MouseButton>>().press(MouseButton::Left);
        app.update();
        assert!(app.world().resource::<Interaction>().drag.is_none());
        assert!(app.world().resource::<Interaction>().split.is_none());
        let mut c=app.world_mut().get_mut::<WindowControls>(inventory).unwrap();
        assert_eq!(c.overflow_pages,2);
        assert!(c.section_visible(Section::OverflowFooter));
        assert!(!c.section_visible(Section::BagFourthRow));
        c.overflow_page=1;
        assert_eq!(place(Cell::Bag(0),&c),Some(Place::Overflow(21)));
    }
}
