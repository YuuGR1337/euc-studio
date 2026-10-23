/*! EUC Thrills — (c) 2026 VibezZzCoder — MIT — https://github.com/VibezZzCoder/EUC-thrills */
import * as THREE from 'three';
import { BLOCKOUT_COLOURS as C, RIDER_BLOCKOUT as B } from './tuning.ts';
import { loftProfile, loftGeometry, loftPoint, loftNormal, vAtHeight, mergeGeometries, shaded, tintOver,
  type LoftProfile, type LoftRing } from './blockoutKit.ts';
import type { RiderLook, RiderPatch, RiderPanelGroup, RiderMaterialSpec } from './riderLook.ts';
import { COOL_REGIONS, createCoolAtlas, type CoolPage, type CoolSheetLayout } from './coolRiderAtlas.ts';

/** Cool Rider's original tailoring and equipment on the shared riding skeleton. */
export const COOL_JACKET = loftProfile([
  { y: -.012, halfWidth: .171, halfDepth: .132, square: 2.6 },
  { y: .003, halfWidth: .173, halfDepth: .134, square: 2.6 },
  { y: .021, halfWidth: .164, halfDepth: .126, square: 2.5 },
  { y: .045, halfWidth: .158, halfDepth: .126, square: 2.4 },
  { y: .079, halfWidth: .155, halfDepth: .123, square: 2.5, z: .001 },
  { y: .102, halfWidth: .152, halfDepth: .126, square: 2.5, z: .004 },
  { y: .133, halfWidth: .146, halfDepth: .119, square: 2.5 },
  { y: .168, halfWidth: .148, halfDepth: .119, square: 2.5, z: .002 },
  { y: .204, halfWidth: .151, halfDepth: .126, square: 2.5, z: .005 },
  { y: .241, halfWidth: .161, halfDepth: .134, square: 2.5, z: .005 },
  { y: .275, halfWidth: .166, halfDepth: .136, square: 2.55, z: .006 },
  { y: .307, halfWidth: .168, halfDepth: .141, square: 2.6, z: .006 },
  { y: .341, halfWidth: .172, halfDepth: .144, square: 2.6, z: .006 },
  { y: .378, halfWidth: .173, halfDepth: .142, square: 2.6, z: .003 },
  { y: .412, halfWidth: .174, halfDepth: .132, square: 2.5 },
  { y: .446, halfWidth: .169, halfDepth: .120, square: 2.4 },
  { y: .475, halfWidth: .158, halfDepth: .107, square: 2.4 },
  { y: .497, halfWidth: .140, halfDepth: .092, square: 2.3 },
  { y: .518, halfWidth: .105, halfDepth: .076, square: 2.2 },
  { y: .535, halfWidth: .074, halfDepth: .068, square: 2.2 },
  { y: .556, halfWidth: .068, halfDepth: .065, square: 2.2 },
  { y: .568, halfWidth: .068, halfDepth: .064, square: 2.2 },
]);
const SEAT = loftProfile([
  { y: -.086, halfWidth: .128, halfDepth: .110, square: 2.5 },
  { y: -.070, halfWidth: .146, halfDepth: .119, square: 2.5 },
  { y: -.042, halfWidth: .163, halfDepth: .128, square: 2.6 },
  { y: -.012, halfWidth: .162, halfDepth: .128, square: 2.6 },
  { y: .025, halfWidth: .155, halfDepth: .118, square: 2.5 },
]);

/** Authored cloth envelopes: calf, knee cup, biceps and cuffs each have their own volume. */
function limb(rows: readonly (readonly [number, number, number, number?])[], square = 2.35): LoftProfile {
  return loftProfile(rows.map(([y, halfWidth, halfDepth, z = 0]) => ({ y, halfWidth, halfDepth, z, square })));
}
const THIGH = limb([
  [-.40,.058,.065],[-.384,.063,.070],[-.361,.071,.075],[-.341,.074,.079],
  [-.318,.071,.075],[-.292,.071,.077],[-.260,.075,.082],[-.235,.077,.086],
  [-.210,.080,.086],[-.176,.083,.087],[-.142,.084,.089],[-.112,.087,.092],
  [-.078,.088,.094],[-.039,.084,.093],[-.008,.081,.086],[.022,.069,.075],
]);
const SHIN = limb([
  [-.38,.044,.044],[-.365,.053,.048],[-.348,.056,.052],[-.330,.050,.050],
  [-.310,.054,.055],[-.287,.052,.055],[-.255,.057,.060,-.002],
  [-.220,.058,.065,-.004],[-.186,.060,.070,-.008],[-.155,.059,.069],
  [-.126,.060,.069,.006],[-.097,.064,.076,.012],[-.062,.069,.082,.014],
  [-.029,.068,.078,.011],[-.008,.061,.069,.007],[.008,.054,.058],
]);
const UPPER_ARM = limb([
  [-.28,.045,.047],[-.262,.050,.052],[-.238,.052,.057],[-.214,.053,.059],
  [-.18,.058,.060],[-.146,.060,.060],[-.107,.063,.062],[-.068,.066,.063],
  [-.035,.067,.061],[-.005,.060,.058],[.020,.041,.042],[.033,.015,.019],
]);
const FOREARM = limb([
  [-.26,.034,.034],[-.248,.041,.038],[-.235,.043,.039],[-.225,.039,.037],
  [-.207,.043,.043],[-.179,.047,.045],[-.151,.050,.049],[-.117,.055,.052],
  [-.082,.057,.056],[-.05,.056,.057],[-.021,.049,.053],[.008,.043,.046],
]);
const NECK = limb([[-.035,.059,.058],[.004,.055,.054],[.046,.052,.052],[.092,.050,.052]],2.2);

export const COOL_HELMET = loftProfile([
  {y:.078,halfWidth:.065,halfDepth:.083,z:.016,square:2.6},
  {y:.090,halfWidth:.090,halfDepth:.105,z:.021,square:2.7},
  {y:.110,halfWidth:.101,halfDepth:.123,z:.023,square:2.8},
  {y:.130,halfWidth:.108,halfDepth:.129,z:.019,square:2.6},
  {y:.150,halfWidth:.113,halfDepth:.127,z:.013,square:2.45},
  {y:.180,halfWidth:.122,halfDepth:.127,z:.005,square:2.4},
  {y:.214,halfWidth:.124,halfDepth:.131,z:0,square:2.35},
  {y:.245,halfWidth:.123,halfDepth:.130,z:-.001,square:2.3},
  {y:.264,halfWidth:.119,halfDepth:.128,z:-.004,square:2.25},
  {y:.282,halfWidth:.109,halfDepth:.119,z:-.006,square:2.2},
  {y:.299,halfWidth:.094,halfDepth:.104,z:-.008,square:2.15},
  {y:.316,halfWidth:.074,halfDepth:.081,z:-.010,square:2.1},
  {y:.329,halfWidth:.050,halfDepth:.055,z:-.011,square:2.05},
  {y:.337,halfWidth:.025,halfDepth:.029,z:-.011,square:2},
  {y:.340,halfWidth:.001,halfDepth:.001,z:-.011,square:2},
]);

function tint(geometry: THREE.BufferGeometry, base: number, target: number, strength = 1): THREE.BufferGeometry {
  const c = geometry.getAttribute('color');
  const t = tintOver(base,target,strength);
  for(let i=0;i<c.count;i++)c.setXYZ(i,t[0],t[1],t[2]);
  return geometry;
}
function ellipsoid(x:number,y:number,z:number,rx:number,ry:number,rz:number,base:number,colour:number,strength=1):THREE.BufferGeometry {
  return tint(shaded(new THREE.SphereGeometry(1,20,12).scale(rx,ry,rz).translate(x,y,z)),base,colour,strength);
}
function cord(points: readonly (readonly [number,number,number])[], radius:number, base:number, colour:number):THREE.BufferGeometry {
  const curve = new THREE.CatmullRomCurve3(points.map(p=>new THREE.Vector3(...p)));
  return tint(shaded(new THREE.TubeGeometry(curve,Math.max(6,points.length*4),radius,6,false)),base,colour);
}

const HELMET_OPENINGS = Object.freeze([0,0,0,0,.58,.85,.92,.89,.68,0,0,0,0,0,0]);

/** The rubber seal and the glass use this same physical cut line. */
function helmetSealCurve(): THREE.CatmullRomCurve3 {
  const points: THREE.Vector3[] = [];
  for (const side of [-1, 1]) {
    const rings = side === -1 ? [3, 4, 5, 6, 7, 8, 9] : [8, 7, 6, 5, 4];
    for (const ring of rings) {
      const point = loftPoint(COOL_HELMET, Math.PI / 2 + side * HELMET_OPENINGS[ring]!, ring, new THREE.Vector3());
      point.z += .001;
      points.push(point);
    }
  }
  return new THREE.CatmullRomCurve3(points, true);
}

/** A double-wall shell with a real front opening, a padded rim for the clear visor. */
export function coolHelmetShell():THREE.BufferGeometry {
  const radial=64, columns=radial+1, rows=COOL_HELMET.length;
  const openings=HELMET_OPENINGS;
  const pos:number[]=[], colours:number[]=[], uv:number[]=[], indices:number[]=[];
  const p=new THREE.Vector3();
  for(let layer=0;layer<2;layer++)for(let r=0;r<rows;r++)for(let c=0;c<columns;c++) {
    const angle=Math.PI/2+openings[r]!+(Math.PI*2-2*openings[r]!)*c/radial;
    loftPoint(COOL_HELMET,angle,r,p);
    const centre=COOL_HELMET[r]!;
    if(layer){p.x*=.94;p.z=(p.z-centre.z)*.94+centre.z;}
    pos.push(p.x,p.y,p.z);uv.push(c/radial,r/(rows-1));
    // Moulded crown channels and rear exhausts use the same shell buffer.
    let shade=layer?.28:1;
    if(!layer&&r>=9&&r<=12&&Math.abs(Math.cos(angle))>.30&&Math.abs(Math.cos(angle))<.45)shade=.62;
    if(!layer&&r>=4&&r<=6&&Math.sin(angle)<-.78)shade=.80;
    colours.push(shade,shade,shade);
  }
  const at=(l:number,r:number,c:number)=>l*rows*columns+r*columns+c;
  const quad=(a:number,b:number,c:number,d:number,reverse=false)=>{
    if(reverse)indices.push(a,c,b,a,d,c);else indices.push(a,b,c,a,c,d);
  };
  for(let r=0;r<rows-1;r++)for(let c=0;c<radial;c++){
    quad(at(0,r,c),at(0,r+1,c),at(0,r+1,c+1),at(0,r,c+1));
    quad(at(1,r,c),at(1,r+1,c),at(1,r+1,c+1),at(1,r,c+1),true);
  }
  for(let r=0;r<rows-1;r++){
    quad(at(0,r,0),at(1,r,0),at(1,r+1,0),at(0,r+1,0));
    quad(at(0,r,radial),at(1,r,radial),at(1,r+1,radial),at(0,r+1,radial),true);
  }
  for(let c=0;c<radial;c++){
    quad(at(0,0,c),at(0,0,c+1),at(1,0,c+1),at(1,0,c));
    quad(at(0,rows-1,c),at(0,rows-1,c+1),at(1,rows-1,c+1),at(1,rows-1,c),true);
  }
  const shell=new THREE.BufferGeometry();shell.setAttribute('position',new THREE.Float32BufferAttribute(pos,3));
  shell.setAttribute('color',new THREE.Float32BufferAttribute(colours,3));shell.setAttribute('uv',new THREE.Float32BufferAttribute(uv,2));
  shell.setIndex(indices);shell.computeVertexNormals();
  const parts=[shell];
  // One closed seal, including the brow and chin tips: no open tube ends.
  const rim = shaded(new THREE.TubeGeometry(helmetSealCurve(), 56, .0035, 6, true));
  parts.push(tint(rim, C.coolHelmet, C.coolInk));
  for (const side of [-1, 1]) {
    parts.push(ellipsoid(side*.121,.241,.009,.006,.014,.014,C.coolHelmet,C.coolGear));
  }
  // Chin intake, integrated into the projecting jaw rather than a floating bar.
  parts.push(ellipsoid(0,.118,.148,.028,.006,.003,C.coolHelmet,C.coolInk));
  return mergeGeometries(parts);
}

/** A single curved pane whose whole cut edge is buried in the rubber seal. */
export function coolVisor(): THREE.BufferGeometry {
  const columns = 40, rows = 25;
  const positions: number[] = [], colors: number[] = [], uvs: number[] = [], indices: number[] = [];
  const seal = helmetSealCurve(), edge = new THREE.Vector3(), centre = new THREE.Vector3();
  const add = (x: number, y: number, z: number, u: number, v: number): void => {
    positions.push(x, y, z); colors.push(1, 1, 1); uvs.push(u, v);
  };
  // One vertex at each tip avoids zero-area faces where the opening closes.
  seal.getPointAt(0, edge); add(0, edge.y, edge.z - .0005, .5, 0);
  for (let row = 1; row < rows; row++) {
    const t = row / rows;
    seal.getPointAt(t * .5, edge);
    loftPoint(COOL_HELMET, Math.PI / 2, vAtHeight(COOL_HELMET, edge.y), centre);
    const rimZ = edge.z - .0005;
    for (let column = 0; column <= columns; column++) {
      const s = column / columns, across = 1 - 2 * s;
      // The centre bows clear of the nose; the offset falls to zero at the
      // actual seal, rather than leaving a separate tinted apron on the shell.
      const bow = .006 * Math.sin(Math.PI * t);
      const z = rimZ + (centre.z + bow - rimZ) * (1 - across * across);
      add(edge.x * across, edge.y, z, s, t);
    }
  }
  const top = positions.length / 3;
  seal.getPointAt(.5, edge); add(0, edge.y, edge.z - .0005, .5, 1);
  const at = (row: number, column: number): number => 1 + (row - 1) * (columns + 1) + column;
  for (let column = 0; column < columns; column++) {
    indices.push(0, at(1, column), at(1, column + 1));
    for (let row = 1; row < rows - 1; row++) {
      const a = at(row, column), b = at(row + 1, column);
      indices.push(a, b, b + 1, a, b + 1, a + 1);
    }
    indices.push(at(rows - 1, column), top, at(rows - 1, column + 1));
  }
  const geometry = new THREE.BufferGeometry();
  geometry.setAttribute('position', new THREE.Float32BufferAttribute(positions, 3));
  geometry.setAttribute('color', new THREE.Float32BufferAttribute(colors, 3));
  geometry.setAttribute('uv', new THREE.Float32BufferAttribute(uvs, 2));
  geometry.setIndex(indices); geometry.computeVertexNormals();
  return geometry;
}

/** Individual tapered hairs, following the lid and curving gently away from the eye. */
function eyelashes(side: number, upper: boolean): THREE.BufferGeometry {
  const parts: THREE.BufferGeometry[] = [];
  const count = upper ? 15 : 12;
  for (let i = 0; i < count; i++) {
    const t = (i + .55) / count, arch = Math.sin(Math.PI * t);
    const x = side * (.018 + .026 * t);
    const y = .220 + (upper ? .0012 + .0022 * arch : -.0012 - .0014 * arch);
    const z = .094 - .006 * t * t;
    // Long natural lashes: subtle irregularity, no exaggerated outer wing or solid lash band.
    const length = (upper ? .0068 : .0048) * (.73 + .27 * arch) * (1 + .13 * Math.sin(i * 2.4));
    const fan = side * (t - .38) * .0024, lift = upper ? .0026 : -.0022;
    const curve = new THREE.QuadraticBezierCurve3(
      new THREE.Vector3(x, y, z),
      new THREE.Vector3(x + fan * .35, y + lift * .2, z + length * .65),
      new THREE.Vector3(x + fan, y + lift, z + length),
    );
    const geometry = shaded(new THREE.TubeGeometry(curve, 5, upper ? .00038 : .00030, 5, false));
    const position = geometry.getAttribute('position');
    const centre = new THREE.Vector3(), vertex = new THREE.Vector3();
    for (let row = 0; row <= 5; row++) {
      const scale = .06 + .94 * (1 - row / 5) ** .8;
      curve.getPointAt(row / 5, centre);
      for (let column = 0; column <= 5; column++) {
        const index = row * 6 + column;
        vertex.fromBufferAttribute(position, index).sub(centre).multiplyScalar(scale).add(centre);
        position.setXYZ(index, vertex.x, vertex.y, vertex.z);
      }
    }
    geometry.computeVertexNormals();
    parts.push(tint(geometry, C.coolSkin, C.coolLash));
  }
  return mergeGeometries(parts);
}

/** Restrained adult facial planes, with a neutral expression beneath the lid. */
export function coolFace():THREE.BufferGeometry {
  const profile=loftProfile([
    {y:.113,halfWidth:.032,halfDepth:.038,z:.038,square:2.5},
    {y:.130,halfWidth:.051,halfDepth:.050,z:.039,square:2.6},
    {y:.150,halfWidth:.063,halfDepth:.061,z:.036,square:2.6},
    {y:.174,halfWidth:.074,halfDepth:.066,z:.031,square:2.45},
    {y:.198,halfWidth:.079,halfDepth:.065,z:.027,square:2.35},
    {y:.220,halfWidth:.076,halfDepth:.065,z:.025,square:2.3},
    {y:.244,halfWidth:.073,halfDepth:.066,z:.022,square:2.25},
    {y:.266,halfWidth:.067,halfDepth:.063,z:.020,square:2.2},
    {y:.286,halfWidth:.049,halfDepth:.048,z:.017,square:2.1},
    {y:.299,halfWidth:0,halfDepth:0,z:.016},
  ]);
  const base=loftGeometry(profile,{radialSegments:40});
  const position=base.getAttribute('position'), color=base.getAttribute('color');
  for(let i=0;i<position.count;i++){
    const y=position.getY(i),x=Math.abs(position.getX(i));
    const k=y<.172?.70+(y-.113)*3.6:1-.12*Math.min(1,x/.079);
    color.setXYZ(i,k,k,k);
  }
  const parts=[base];
  // Nose: a bridge, a tip and small alar wings. No painted feature floats off the face.
  parts.push(loftGeometry(loftProfile([
    {y:.174,halfWidth:.009,halfDepth:.007,z:.101,square:2.5},
    {y:.182,halfWidth:.014,halfDepth:.013,z:.108,square:2.4},
    {y:.190,halfWidth:.012,halfDepth:.016,z:.110,square:2.3},
    {y:.207,halfWidth:.008,halfDepth:.010,z:.102,square:2.3},
    {y:.231,halfWidth:.006,halfDepth:.006,z:.093,square:2.2},
  ]),{radialSegments:20}));
  for(const side of [-1,1]){
    parts.push(ellipsoid(side*.012,.180,.108,.007,.004,.006,C.coolSkin,C.coolSkinShadow));
    parts.push(ellipsoid(side*.031,.220,.091,.013,.0028,.0025,C.coolSkin,0x969187));
    parts.push(ellipsoid(side*.031,.220,.0935,.0031,.0027,.001,C.coolSkin,C.coolInk));
    parts.push(cord([[side*.016,.222,.094],[side*.031,.224,.094],[side*.045,.222,.088]],.0013,C.coolSkin,C.coolSkinShadow));
    parts.push(eyelashes(side, true), eyelashes(side, false));
    parts.push(cord([[side*.014,.237,.094],[side*.029,.239,.094],[side*.045,.237,.087],[side*.052,.234,.080]],.0015,C.coolSkin,C.coolInk));
  }
  parts.push(cord([[-.019,.153,.100],[0,.152,.104],[.019,.153,.100]],.0013,C.coolSkin,C.coolSkinShadow));
  parts.push(ellipsoid(0,.149,.100,.015,.002,.002,C.coolSkin,C.coolSkin,.79));
  return mergeGeometries(parts);
}

const BOOT=loftProfile([
  {y:-.098,halfWidth:.027,halfDepth:.030,square:2.4},
  {y:-.081,halfWidth:.046,halfDepth:.044,square:2.5},
  {y:-.059,halfWidth:.050,halfDepth:.060,z:-.010,square:2.5},
  {y:-.035,halfWidth:.055,halfDepth:.066,z:-.016,square:2.5},
  {y:-.009,halfWidth:.059,halfDepth:.052,z:-.009,square:2.5},
  {y:.024,halfWidth:.063,halfDepth:.037,square:2.6},
  {y:.058,halfWidth:.063,halfDepth:.031,square:2.7},
  {y:.091,halfWidth:.058,halfDepth:.026,square:2.8},
  {y:.119,halfWidth:.045,halfDepth:.021,square:2.7},
  {y:.136,halfWidth:.027,halfDepth:.015,square:2.5},
  {y:.143,halfWidth:0,halfDepth:0},
].map(ring => ({ ...ring, z: .047 - ring.halfDepth })));
const SOLE=loftProfile([
  {y:-.018,halfWidth:.055,halfDepth:.116,square:4},
  {y:-.015,halfWidth:.061,halfDepth:.124,square:4.6},
  {y:-.005,halfWidth:.064,halfDepth:.128,square:4.6},
  {y:0,halfWidth:.061,halfDepth:.125,square:4.2},
]);
function bootDetail():THREE.BufferGeometry {
  const parts:THREE.BufferGeometry[]=[];
  // Shoe laces follow the upper's sampled top, in the same ankle frame as the boot.
  const at=(x:number,z:number):[number,number,number]=>{
    const p=new THREE.Vector3();loftPoint(BOOT,Math.PI*1.5,vAtHeight(BOOT,z),p);
    return [x,-B.ankleAbovePedal+.018+.047-p.z+.0015,z];
  };
  for(let n=0;n<5;n++){
    const z=.040+n*.013;
    parts.push(cord([at(-.025,z),at(0,z+.004),at(.025,z+.009)],.0018,C.coolGear,C.coolStitch));
    parts.push(cord([at(.025,z),at(0,z+.005),at(-.025,z+.009)],.0018,C.coolGear,C.coolStitch));
  }
  return mergeGeometries(parts);
}
function paintBoot(g:THREE.BufferGeometry):void {
  const p=g.getAttribute('position'),c=g.getAttribute('color');
  const sole=tintOver(C.coolGear,C.coolSole),tread=tintOver(C.coolGear,C.coolInk);
  for(let i=0;i<p.count;i++){
    if(p.getY(i)<=-.0415){const t=p.getY(i)<-.055?tread:sole;c.setXYZ(i,...t);}
  }
}

const HAND=limb([[-.080,.029,.019],[-.068,.037,.023],[-.050,.038,.025],[-.033,.035,.025],[-.026,.036,.029],[-.011,.038,.031],[.007,.034,.029]],2.65);
function fingers(side:number):THREE.BufferGeometry {
  const parts:THREE.BufferGeometry[]=[];
  for(let n=0;n<4;n++){
    const x=(n-1.5)*.017, length=[.034,.042,.039,.029][n]!;
    const ring=(y:number,w:number,d:number,z:number):LoftRing=>({y,halfWidth:w,halfDepth:d,x,z,square:2.3});
    const g=loftGeometry(loftProfile([
      ring(-.067,.008,.012,.004),ring(-.080,.008,.012,.002),
      ring(-.088,.0075,.011,-.004),ring(-.078-length,.007,.008,-.012),
      ring(-.083-length,0,0,-.013),
    ]),{radialSegments:12});
    const p=g.getAttribute('position'),c=g.getAttribute('color');const t=tintOver(C.coolGear,C.coolSkin);
    for(let i=0;i<p.count;i++)if(p.getY(i)<-.087)c.setXYZ(i,...t);
    parts.push(g);
  }
  const thumb=loftGeometry(loftProfile([
    {y:-.032,halfWidth:.012,halfDepth:.013,x:-side*.030,z:.002,square:2.3},
    {y:-.047,halfWidth:.013,halfDepth:.014,x:-side*.041,z:.001,square:2.3},
    {y:-.063,halfWidth:.011,halfDepth:.013,x:-side*.045,z:-.006,square:2.3},
    {y:-.079,halfWidth:.008,halfDepth:.009,x:-side*.043,z:-.012,square:2.2},
    {y:-.086,halfWidth:0,halfDepth:0,x:-side*.041,z:-.014},
  ]),{radialSegments:16});
  const p=thumb.getAttribute('position'),c=thumb.getAttribute('color'),t=tintOver(C.coolGear,C.coolSkin);
  for(let i=0;i<p.count;i++)if(p.getY(i)<-.062)c.setXYZ(i,...t);
  parts.push(thumb);
  // Separate knuckle pads, kept inside the glove's silhouette.
  for(let n=0;n<4;n++)parts.push(ellipsoid((n-1.5)*.016,-.058,.023,.007,.009,.005,C.coolGear,C.coolInk));
  return mergeGeometries(parts);
}

const fabric:RiderMaterialSpec=Object.freeze({colour:C.coolFabric,roughness:.88,metalness:0});
/** The tailoring sheet's lofts (exported for the atlas painter's byte pin). */
export const COOL_SHEET_LAYOUT:CoolSheetLayout=Object.freeze({jacket:COOL_JACKET,thigh:THIGH,shin:SHIN,sleeve:UPPER_ARM,forearm:FOREARM,seat:SEAT});
const blue=(patch:Partial<RiderPatch>&Pick<RiderPatch,'anchor'|'u0'|'u1'|'from'|'to'>):RiderPatch=>Object.freeze({
  uSegments:14,vSegments:18,lift:.0035,sink:-.006,art:'blue',...patch,
});
const group=(patches:readonly RiderPatch[],casts=false):RiderPanelGroup=>Object.freeze({role:'accent',casts,patches:Object.freeze(patches)});

/** The curved flank is one sewn panel, sampled on the jacket with no overlapping slabs. */
export function coolFlank(side:number):THREE.BufferGeometry {
  const knots=[
    [.042,.86,.07],[.075,.84,.115],[.15,.86,.13],[.24,.93,.14],
    [.32,.92,.145],[.39,.83,.16],[.445,.68,.14],[.480,.60,.075],[.496,.57,.018],
  ];
  const rows=72,cols=14,count=(rows+1)*(cols+1);
  const positions:number[]=[],colours:number[]=[],uv:number[]=[],index:number[]=[];
  const p=new THREE.Vector3(),n=new THREE.Vector3();
  for(let layer=0;layer<2;layer++)for(let r=0;r<=rows;r++){
    const t=r/rows,y=knots[0]![0]!+t*(knots.at(-1)![0]!-knots[0]![0]!);
    let k=0;while(k<knots.length-2&&y>knots[k+1]![0]!)k++;
    const a=knots[k]!,b=knots[k+1]!,f=(y-a[0]!)/(b[0]!-a[0]!);
    const eased=f*f*(3-2*f),centre=a[1]!+(b[1]!-a[1]!)*eased,width=a[2]!+(b[2]!-a[2]!)*eased;
    for(let c=0;c<=cols;c++){
      const s=c/cols,u=Math.PI/2+side*(centre+(s*2-1)*width),v=vAtHeight(COOL_JACKET,y);
      loftPoint(COOL_JACKET,u,v,p);loftNormal(COOL_JACKET,u,v,n);p.addScaledVector(n,layer?-.004:.0025);
      positions.push(p.x,p.y,p.z);colours.push(1,1,1);uv.push(s,t);
    }
  }
  const at=(l:number,r:number,c:number)=>l*count+r*(cols+1)+c;
  const quad=(a:number,b:number,c:number,d:number,flip=false)=>{
    if((side<0)!==flip)index.push(a,c,b,a,d,c);else index.push(a,b,c,a,c,d);
  };
  for(let r=0;r<rows;r++)for(let c=0;c<cols;c++){
    quad(at(0,r,c),at(0,r+1,c),at(0,r+1,c+1),at(0,r,c+1));
    quad(at(1,r,c),at(1,r+1,c),at(1,r+1,c+1),at(1,r,c+1),true);
  }
  for(let r=0;r<rows;r++){
    quad(at(0,r,0),at(1,r,0),at(1,r+1,0),at(0,r+1,0));
    quad(at(0,r,cols),at(1,r,cols),at(1,r+1,cols),at(0,r+1,cols),true);
  }
  for(let c=0;c<cols;c++){
    quad(at(0,0,c),at(0,0,c+1),at(1,0,c+1),at(1,0,c));
    quad(at(0,rows,c),at(0,rows,c+1),at(1,rows,c+1),at(1,rows,c),true);
  }
  const g=new THREE.BufferGeometry();g.setAttribute('position',new THREE.Float32BufferAttribute(positions,3));
  g.setAttribute('color',new THREE.Float32BufferAttribute(colours,3));g.setAttribute('uv',new THREE.Float32BufferAttribute(uv,2));
  g.setIndex(index);g.computeVertexNormals();return g;
}

export const COOL_RIDER_LOOK:RiderLook=Object.freeze({
  id:'cool-rider',
  materials:Object.freeze({body:fabric,limbs:fabric,
    accent:Object.freeze({colour:C.coolPrint,roughness:.78,metalness:0}),
    head:Object.freeze({colour:C.coolHelmet,roughness:.27,metalness:0}),
    face:Object.freeze({colour:C.coolSkin,roughness:.78,metalness:0}),
    gear:Object.freeze({colour:C.coolGear,roughness:.67,metalness:0}),
  }),
  profiles:Object.freeze({torso:COOL_JACKET,seat:SEAT,thigh:THIGH,shin:SHIN,upperArm:UPPER_ARM,
    forearm:FOREARM,neck:NECK,head:COOL_HELMET,boot:BOOT,bootSole:SOLE,hand:HAND}),
  density:Object.freeze({torso:48,limb:32,head:64,boot:32,hand:24,neck:24}),
  shades:Object.freeze({seat:.90,legs:.94,collar:1,sole:1,neck:.6}),
  parts:Object.freeze({hands:'gear',neck:'gear',kneePad:'accent',legs:'limbs',seat:'body'}),
  atlas:Object.freeze({build:()=>createCoolAtlas(COOL_SHEET_LAYOUT),roles:Object.freeze(['body','limbs','accent'] as const),
    region:(art:string|undefined)=>COOL_REGIONS[art as CoolPage]??COOL_REGIONS.blank,
    lofts:Object.freeze({torso:'jacket',seat:'seat',thigh:'thigh',shin:'shin',upperArm:'sleeve',forearm:'forearm'}),
  }),
  build:Object.freeze({head:coolHelmetShell,hand:Object.freeze([fingers]),boot:Object.freeze([bootDetail])}),
  paint:Object.freeze({boot:paintBoot}),
  panels:Object.freeze({
    torso:Object.freeze({...group([
      // The familiar chest chevrons become narrow piping within the tailored jacket.
      blue({anchor:'front',u0:.10,u1:.59,mirrored:true,from:.311,to:.324,skewFrom:.34,skewTo:.37,taper:.12}),
      // The familiar blue rear field, now thin and tailored to the shaped back.
      blue({anchor:'back',u0:-.64,u1:.64,from:.224,to:.457,taper:.32}),
    ]),build:coolFlank,art:'blue'}),
    sleeve:group([blue({anchor:'outboard',u0:-.75,u1:.60,from:-.264,to:-.017,taper:.22})]),
    elbowPad:group([
      blue({anchor:'outboard',u0:-.55,u1:.48,from:-.231,to:-.014,taper:.13}),
      blue({anchor:'back',u0:-.6,u1:.6,from:-.093,to:-.018,taper:.36,lift:.004,art:'guard'}),
    ]),
    kneePad:group([blue({anchor:'front',u0:-.86,u1:.86,from:-.137,to:-.013,taper:.35,lift:.0045,art:'guard'})]),
    thighPad:Object.freeze({role:'body' as const,casts:false,patches:Object.freeze([
      blue({anchor:'outboard',u0:-.66,u1:.66,from:-.232,to:-.070,taper:.17,lift:.003,art:'pocket'}),
    ])}),
    head:Object.freeze([]),
  }),
  extras:Object.freeze([
    {name:'rider-cool-face',joint:'neck' as const,role:'face' as const,casts:true,build:coolFace},
    {name:'rider-cool-visor',joint:'neck' as const,role:'face' as const,casts:false,build:coolVisor,
      material:Object.freeze({colour:C.coolVisor,roughness:.13,metalness:0,opacity:.20})},
  ]),
  armCarriage:Object.freeze({splay:0,rise:0}),
});
