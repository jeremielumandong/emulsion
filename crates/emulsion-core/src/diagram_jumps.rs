//! Editable line jumps. Spatial queries limit crossing tests to nearby segments.
use super::*;
use rstar::{AABB, RTree, RTreeObject};
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JumpStyle {
    #[default]
    None,
    Arc,
    Gap,
    Sharp,
}
impl JumpStyle {
    pub fn drawio(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Arc => "arc",
            Self::Gap => "gap",
            Self::Sharp => "sharp",
        }
    }
}
#[derive(Clone)]
struct Segment {
    edge: u64,
    a: (f64, f64),
    b: (f64, f64),
}
impl RTreeObject for Segment {
    type Envelope = AABB<[f64; 2]>;
    fn envelope(&self) -> Self::Envelope {
        AABB::from_corners(
            [self.a.0.min(self.b.0), self.a.1.min(self.b.1)],
            [self.a.0.max(self.b.0), self.a.1.max(self.b.1)],
        )
    }
}
fn cross(a: (f64, f64), b: (f64, f64)) -> f64 {
    a.0 * b.1 - a.1 * b.0
}
fn subtract(a: (f64, f64), b: (f64, f64)) -> (f64, f64) {
    (a.0 - b.0, a.1 - b.1)
}
#[cfg(test)]
fn intersection(a: &Segment, b: &Segment) -> Option<f64> {
    let r = subtract(a.b, a.a);
    let s = subtract(b.b, b.a);
    let d = cross(r, s);
    if d.abs() < 1e-9 {
        return None;
    }
    let delta = subtract(b.a, a.a);
    let t = cross(delta, s) / d;
    let u = cross(delta, r) / d;
    ((1e-6..1. - 1e-6).contains(&t) && (1e-6..1. - 1e-6).contains(&u)).then_some(t * r.0.hypot(r.1))
}
type Curve=[(f64,f64);4];
fn at(c:Curve,t:f64)->(f64,f64){emulsion_raster::vector::cubic_at(c[0],c[1],c[2],c[3],t)}
fn mix(a:(f64,f64),b:(f64,f64),t:f64)->(f64,f64){(a.0+(b.0-a.0)*t,a.1+(b.1-a.1)*t)}
fn split(c:Curve,t:f64)->(Curve,Curve){
    let a=mix(c[0],c[1],t);let b=mix(c[1],c[2],t);let d=mix(c[2],c[3],t);
    let e=mix(a,b,t);let f=mix(b,d,t);let p=mix(e,f,t);
    ([c[0],a,e,p],[p,f,d,c[3]])
}
fn section(c:Curve,a:f64,b:f64)->Curve {
    if a==0. && b==1. {return c;}
    let (left,_)=split(c,b);let (_,part)=split(left,if b>0.{a/b}else{0.});part
}
fn append(line:&mut SubPath,c:Curve){
    if let Some(last)=line.anchors.last_mut(){last.h_out=c[1];}
    let mut last=Anchor::corner(c[3]);last.h_in=c[2];line.anchors.push(last);
}
fn sample(c:Curve)->Vec<(f64,(f64,f64))>{
    fn recurse(c:Curve,a:f64,b:f64,depth:u32,out:&mut Vec<(f64,(f64,f64))>){
        let chord=subtract(c[3],c[0]);let len=chord.0.hypot(chord.1);
        let flat=if len<1e-9 {c.iter().all(|p|subtract(*p,c[0]).0.hypot(subtract(*p,c[0]).1)<0.1)}else{
            c[1..3].iter().all(|p| cross(subtract(*p,c[0]),chord).abs()/len<0.1)
                && c[1..3].iter().all(|p|{let d=subtract(*p,c[0]);let projection=(d.0*chord.0+d.1*chord.1)/(len*len);(0. ..=1.).contains(&projection)})
        };
        let parameter_flat=[0.25,0.75].iter().all(|t|{let p=at(c,*t);let linear=mix(c[0],c[3],*t);(p.0-linear.0).hypot(p.1-linear.1)<0.1});
        if depth>=14 || (flat && parameter_flat) {out.push((b,c[3]));return;}
        let (left,right)=split(c,0.5);let mid=(a+b)/2.;recurse(left,a,mid,depth+1,out);recurse(right,mid,b,depth+1,out);
    }
    let mut out=vec![(0.,c[0])];recurse(c,0.,1.,0,&mut out);out
}
fn intersection_inclusive(a:&Segment,b:&Segment)->Option<f64>{
    let r=subtract(a.b,a.a);let s=subtract(b.b,b.a);let d=cross(r,s);
    if d.abs()<1e-9{return None;}
    let delta=subtract(b.a,a.a);let t=cross(delta,s)/d;let u=cross(delta,r)/d;
    ((-1e-8..=1.+1e-8).contains(&t) && (-1e-8..=1.+1e-8).contains(&u)).then_some(t.clamp(0.,1.)*r.0.hypot(r.1))
}
pub(super) fn apply(doc: &mut Document, model: &Diagram) {
    if !model
        .edges
        .values()
        .any(|e| e.jump_style != JumpStyle::None)
    {
        return;
    }
    let indices = doc
        .nodes
        .iter()
        .enumerate()
        .map(|(i, n)| (n.id, i))
        .collect::<HashMap<_, _>>();
    let paths = model
        .edges
        .iter()
        .filter_map(|(id, e)| match &doc.nodes[indices[&e.path]].kind {
            NodeKind::Path { path, .. } => Some((*id, path.clone())),
            _ => None,
        })
        .collect::<HashMap<_, _>>();
    let segments = paths
        .iter()
        .flat_map(|(id, p)| {
            p.flatten(0.5).into_iter().flat_map(move |(points, _)| {
                points
                    .windows(2)
                    .map(|pair| Segment {
                        edge: *id,
                        a: pair[0],
                        b: pair[1],
                    })
                    .collect::<Vec<_>>()
            })
        })
        .collect();
    let tree = RTree::bulk_load(segments);
    for (id, edge) in &model.edges {
        if edge.jump_style == JumpStyle::None { continue; }
        let Some(path)=paths.get(id) else {continue;};
        let mut result=Path::default();
        for sub in &path.subpaths {
            if sub.closed || sub.anchors.len()<2 {result.subpaths.push(sub.clone());continue;}
            let mut line=SubPath{anchors:vec![sub.anchors[0]],closed:false};
            for pair in sub.anchors.windows(2) {
                let curve=[pair[0].p,pair[0].h_out,pair[1].h_in,pair[1].p];
                let samples=sample(curve);
                let mut distances=vec![0.];
                for p in samples.windows(2) {
                    distances.push(distances.last().unwrap()+subtract(p[1].1,p[0].1).0.hypot(subtract(p[1].1,p[0].1).1));
                }
                let length=*distances.last().unwrap();
                if length<1e-9 {line.anchors.push(pair[1]);continue;}
                let radius=(edge.jump_size/2.).min(length/4.);
                let mut hits=Vec::new();
                for (i,p) in samples.windows(2).enumerate() {
                    let segment=Segment{edge:*id,a:p[0].1,b:p[1].1};
                    for other in tree.locate_in_envelope_intersecting(&segment.envelope()).filter(|s|s.edge!=*id && (s.edge<*id || model.edges[&s.edge].jump_style==JumpStyle::None)) {
                        if let Some(distance)=intersection_inclusive(&segment,other) {
                            let d=distances[i]+distance;
                            if d>radius && d<length-radius {hits.push(d);}
                        }
                    }
                }
                hits.sort_by(f64::total_cmp);
                hits.dedup_by(|a,b|(*a-*b).abs()<radius*2.);
                let parameter=|distance:f64| {
                    let i=distances.partition_point(|v|*v<distance).clamp(1,distances.len()-1);
                    let ratio=(distance-distances[i-1])/(distances[i]-distances[i-1]).max(1e-12);
                    samples[i-1].0+(samples[i].0-samples[i-1].0)*ratio
                };
                let mut from=0.;
                for distance in hits {
                    let a=parameter(distance-radius);let b=parameter(distance+radius);
                    append(&mut line,section(curve,from,a));
                    let start=line.anchors.last().unwrap().p;
                    let end=at(curve,b);
                    let delta=subtract(end,start);let chord=delta.0.hypot(delta.1).max(1e-9);
                    let normal=(delta.1/chord*radius,-delta.0/chord*radius);
                    match edge.jump_style {
                        JumpStyle::Gap=>{result.subpaths.push(line);line=SubPath{anchors:vec![Anchor::corner(end)],closed:false};}
                        JumpStyle::Sharp=>{
                            line.anchors.push(Anchor::corner(((start.0+end.0)/2.+normal.0,(start.1+end.1)/2.+normal.1)));
                            line.anchors.push(Anchor::corner(end));
                        }
                        JumpStyle::Arc=>{
                            line.anchors.last_mut().unwrap().h_out=(start.0+normal.0*4./3.,start.1+normal.1*4./3.);
                            let mut anchor=Anchor::corner(end);anchor.h_in=(end.0+normal.0*4./3.,end.1+normal.1*4./3.);line.anchors.push(anchor);
                        }
                        JumpStyle::None=>{}
                    }
                    from=b;
                }
                append(&mut line,section(curve,from,1.));
            }
            result.subpaths.push(line);
        }
        let (w, h) = (doc.width, doc.height);
        if let NodeKind::Path { path, style, cache } = &mut doc.nodes[indices[&edge.path]].kind
            && **path != result
        {
            *path = Arc::new(result);
            *cache = crate::vector_cache::VectorRaster::path(path.clone(), *style, w, h);
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn crossing_excludes_endpoints_and_parallel_lines() {
        let a = Segment {
            edge: 1,
            a: (0., 0.),
            b: (100., 0.),
        };
        assert_eq!(
            intersection(
                &a,
                &Segment {
                    edge: 2,
                    a: (50., -50.),
                    b: (50., 50.)
                }
            ),
            Some(50.)
        );
        assert_eq!(
            intersection(
                &a,
                &Segment {
                    edge: 2,
                    a: (100., 0.),
                    b: (100., 50.)
                }
            ),
            None
        );
        assert_eq!(
            intersection(
                &a,
                &Segment {
                    edge: 2,
                    a: (0., 10.),
                    b: (100., 10.)
                }
            ),
            None
        );
    }
}
