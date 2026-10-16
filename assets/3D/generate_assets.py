#!/usr/bin/env python3
"""Motor-Trail procedural low-poly environment prototype.

Generates original geometry, one self-contained GLB per asset, a preview sheet,
and a lightweight metadata catalog. Requires numpy, trimesh, Pillow.
Conventions: working geometry Z-up, glTF exporter converts to glTF Y-up.
"""
import json
import math
import pathlib
import random
import zipfile
from collections import defaultdict
from typing import List, Tuple

import numpy as np
import trimesh
from PIL import Image, ImageDraw, ImageFont

ROOT=pathlib.Path(__file__).resolve().parent
OUT=ROOT/'assets'
OUT.mkdir(exist_ok=True)
SEED=26016

PALETTE={
 'leaf_dark':'#276B3C', 'leaf_mid':'#3C9652', 'leaf_light':'#65A665', 'leaf_sage':'#8DA65D',
 'leaf_deep':'#245D37', 'pine_dark':'#305F48', 'pine_mid':'#438260', 'pine_light':'#59966A',
 'bark':'#87613F', 'bark_light':'#9E764B', 'bark_dark':'#63462F',
 'rock_mid':'#829297', 'rock_light':'#BAC0B5', 'rock_dark':'#64777D', 'rock_warm':'#9A9687',
 'grass':'#579C4E','grass_light':'#80B65E','grass_dark':'#347A45',
 'fern':'#3D9856','fern_light':'#71B568', 'flower':'#D9A760', 'flower_pink':'#D78C99', 'center':'#DAAA4C',
 'dead_bark':'#8B755D'
}

def rgba(h):
 h=h.lstrip('#')
 return [int(h[i:i+2],16) for i in (0,2,4)]+[255]

MATS={k:trimesh.visual.material.PBRMaterial(name=k,baseColorFactor=rgba(v),roughnessFactor=1.0,metallicFactor=0.,doubleSided=True) for k,v in PALETTE.items()}

class Model:
 def __init__(self, name, category, desc):
  self.name=name; self.category=category; self.desc=desc
  self.parts=[]
 def add(self,mesh,material):
  if mesh is None or len(mesh.faces)==0:return
  mesh=mesh.copy()
  mesh.unmerge_vertices()  # split normals at every facet: genuinely flat-shaded in game engines
  mesh.visual=trimesh.visual.TextureVisuals(material=MATS[material])
  self.parts.append((mesh,material))
 def scene(self):
  # Merge disconnected surfaces that use the same material to reduce GPU draw calls.
  scene=trimesh.Scene()
  groups=defaultdict(list)
  for mesh,mat in self.parts:groups[mat].append(mesh)
  for mat,meshes in groups.items():
   merged=trimesh.util.concatenate(meshes)
   merged.visual=trimesh.visual.TextureVisuals(material=MATS[mat])
   scene.add_geometry(merged,geom_name=f'{self.name}_{mat}')
  return scene
 def save(self):
  file=OUT/f'{self.name}.glb'
  file.write_bytes(self.scene().export(file_type='glb'))
  mins,maxs=self.bounds()
  return dict(name=self.name,category=self.category,description=self.desc,file=f'assets/{file.name}',triangles=sum(len(m.faces) for m,_ in self.parts),materials=sorted(set(t for _,t in self.parts)),size_m=[round(float(x),2) for x in maxs-mins])
 def bounds(self):
  v=np.concatenate([m.vertices for m,_ in self.parts],axis=0)
  return v.min(axis=0), v.max(axis=0)

def rng_for(name):return random.Random(SEED+sum((i+1)*ord(c) for i,c in enumerate(name)))

def cyl(start, end, r0, r1, sides=7, phase=0):
 start=np.asarray(start,float);end=np.asarray(end,float)
 v=end-start;z=v/np.linalg.norm(v)
 ref=np.array([0.,0.,1.]) if abs(z[2])<.85 else np.array([0.,1.,0.])
 x=np.cross(z,ref);x/=np.linalg.norm(x);y=np.cross(z,x)
 verts=[]
 for center,r in ((start,r0),(end,r1)):
  for k in range(sides):
   a=phase+2*math.pi*k/sides
   verts.append(center+r*(math.cos(a)*x+math.sin(a)*y))
 verts += [start,end]
 faces=[]
 for k in range(sides):
  j=(k+1)%sides
  faces.extend([[k,j,sides+j],[k,sides+j,sides+k],[2*sides,j,k],[2*sides+1,sides+k,sides+j]])
 return trimesh.Trimesh(vertices=verts,faces=faces,process=False)

def sphere(center,radii,seed,sub=1):
 r=random.Random(seed)
 m=trimesh.creation.icosphere(subdivisions=sub,radius=1)
 v=m.vertices.copy()
 v*=np.array([r.uniform(.90,1.10) for _ in range(len(v))])[:,None]
 v[:,0]*=radii[0];v[:,1]*=radii[1];v[:,2]*=radii[2]
 v+=np.array(center)
 return trimesh.Trimesh(vertices=v,faces=m.faces,process=False)

def irregular_rock(w,d,h,seed,plateau=False):
 r=random.Random(seed)
 m=trimesh.creation.icosphere(subdivisions=1,radius=1.0)
 v=m.vertices.copy()
 for k in range(len(v)):
  j=r.uniform(.78,1.13)
  v[k]*=j
 v[:,0]*=w/2;v[:,1]*=d/2;v[:,2]*=h/2
 if plateau:
  v[:,2]=np.minimum(v[:,2],h*.24)
 v[:,2]-=v[:,2].min()
 # Flat-ish base by truncation: keep geometry manifold, then floor lowest ring.
 v[v[:,2] < .10*h,2]=0
 return trimesh.Trimesh(vertices=v,faces=m.faces,process=False)

def rock_parts(model,mesh,base='rock_mid',accents=('rock_light','rock_dark'),salt=0):
 # Split contiguous-ish faces into three small PBR material groups.
 norms=mesh.face_normals
 for mat in (base,)+tuple(accents):
  picks=[]
  for i,n in enumerate(norms):
   # deterministic broadly separates flat lit facets and a few faces
   a=((i*13+salt*7)%23)
   typ = base if a<18 else (accents[0] if n[2]>.2 else accents[1])
   if typ==mat:picks.append(i)
  if picks:
   sub=trimesh.Trimesh(vertices=mesh.vertices,faces=mesh.faces[picks],process=False)
   model.add(sub,mat)

def leaf_blade(center,angle,h,width,lean=.2):
 x,y,z=center
 dx,dy=math.cos(angle),math.sin(angle)
 side=np.array([-dy,dx,0.])
 tip=np.array([x+dx*lean*h,y+dy*lean*h,z+h])
 left=np.array([x,y,z])-side*width/2
 right=np.array([x,y,z])+side*width/2
 mid=np.array([x+dx*lean*h*.40,y+dy*lean*h*.40,z+h*.37])+np.array([0.,0.,0.])
 vertices=np.array([left,mid,right,tip])
 return trimesh.Trimesh(vertices=vertices,faces=[[0,1,3],[1,2,3]],process=False)

def make_broadleaf(name, height, canopy_w, crown, seed):
 r=rng_for(name);m=Model(name,'trees','Arbre feuillu à houppier facetté')
 shift=(r.uniform(-.32,.32),r.uniform(-.19,.19))
 trunk_h=height*r.uniform(.44,.53)
 m.add(cyl([0,0,0],[shift[0],shift[1],trunk_h],height*.047,height*.026,7,r.random()),'bark')
 headbase=np.array([shift[0],shift[1],trunk_h])
 offsets=[(0,0,.09, .55),(-.30,-.04,-.01,.43),(.31,.10,.02,.42),(-.02,.23,.24,.34),(.09,-.22,.20,.38)]
 if crown=='tall':offsets=[(0,0,0,.56),(-.25,0,-.14,.40),(.29,.07,.14,.42),(0,0,.35,.38)]
 if crown=='wide':offsets += [(-.48,.1,-.15,.33),(.49,-.06,-.10,.35)]
 colors=['leaf_mid','leaf_dark','leaf_light','leaf_sage','leaf_mid','leaf_dark','leaf_light']
 for j,(ox,oy,oz,s) in enumerate(offsets):
  center=headbase+np.array([ox*canopy_w,oy*canopy_w,oz*height+ s*height*.38])
  if j:
   branch= headbase + np.array([ox*canopy_w*.66,oy*canopy_w*.66,oz*height*.8])
   m.add(cyl(headbase,branch,height*.020,height*.008,5,j),'bark_dark')
  m.add(sphere(center, (canopy_w*s,canopy_w*s*.89,height*s*.33),seed+j*10), colors[j])
 return m

def make_pine(name,height,width,seed,sections):
 r=rng_for(name);m=Model(name,'trees','Conifère en étages anguleux')
 m.add(cyl([0,0,0],[0,0,height*.89],width*.10,width*.023,6,.1),'bark_dark')
 for k in range(sections):
  lower=.19+k*.19
  apex=min(.99,lower+.40)
  radius=width/2*(1-k/(sections+1)*.65)
  mesh=cyl([0,0,height*lower],[r.uniform(-.04,.04),0,height*apex],radius,0.015,7,.3+k*.4)
  m.add(mesh,['pine_dark','pine_mid','pine_light','pine_mid'][k%4])
 return m

def make_bush(name,w,h,seed,choice):
 r=rng_for(name);m=Model(name,'bushes','Buisson anguleux multi-volumes')
 num=choice
 cols=['leaf_mid','leaf_dark','leaf_light','leaf_sage']
 for j in range(num):
  a=j*2.399+0.25
  rr=w*.23*(.3+.7*((j*5)%7)/6)
  x=math.cos(a)*rr; y=math.sin(a)*rr
  size=w*r.uniform(.22,.34)
  rh=h*r.uniform(.34,.60)
  z=rh+ h*r.uniform(.05,.20)
  m.add(sphere((x,y,z),(size,size*.87,rh),seed+j*11),cols[j%len(cols)])
 return m

def make_grass(name,num,h,seed):
 r=rng_for(name);m=Model(name,'plants','Touffe d’herbe stylisée')
 for j in range(num):
  a=j*2.399+r.uniform(-.15,.15)
  rr=.07+r.random()*.13
  c=(rr*math.cos(a),rr*math.sin(a),0)
  m.add(leaf_blade(c,a,h*r.uniform(.57,1),h*r.uniform(.09,.16),r.uniform(.25,.46)),['grass','grass_light','grass_dark'][j%3])
 return m

def make_fern(name,fronds,size,seed):
 r=rng_for(name);m=Model(name,'plants','Fougère polygonale à frondes segmentées')
 # Each frond has 4-6 angled geometric leaf segments.
 for f in range(fronds):
  a=2*math.pi*f/fronds+.3
  ux,uy=math.cos(a),math.sin(a)
  st=np.array([0.,0.,.04]);end=np.array([ux*size*.74,uy*size*.74,size*.78])
  m.add(cyl(st,end,.017,.005,4),'grass_dark')
  across=np.array([-uy,ux,0])
  for t in range(1,5):
   k=t/5
   mid=st*(1-k)+end*k
   span=size*.20*(1-k*.45)
   for s in (-1,1):
    v0=mid+across*.025*s
    v1=mid+across*span*s+np.array([ux*size*.06,uy*size*.06,-size*.13])
    v2=mid+np.array([ux*size*.14,uy*size*.14,size*.09])
    leaf=trimesh.Trimesh(vertices=[v0,v1,v2],faces=[[0,1,2]],process=False)
    m.add(leaf,['fern','fern_light'][(t+s)//2%2])
 return m

def make_log(name,length,radius,seed,stump=False):
 r=rng_for(name);m=Model(name,'wood','Bois mort / obstacle décoratif')
 if stump:
  base=np.array([0.,0.,0]);top=np.array([0.,0.,length])
  m.add(cyl(base,top,radius*1.10,radius*.95,8,r.random()),'bark')
  m.add(cyl(top-[0,0,.008],top+[0,0,.008],radius*.92,radius*.92,8),'bark_light')
 else:
  start=np.array([-length/2,0,radius*.95]);end=np.array([length/2,radius*.07,radius*.86])
  m.add(cyl(start,end,radius,radius*.91,9,r.random()),'bark')
  m.add(cyl(end,end+[.018,0,0],radius*.90,radius*.90,9),'bark_light')
  # small broken branch on top
  mid=start+(end-start)*.65
  m.add(cyl(mid,mid+[.13,0,radius*.9],radius*.17,radius*.05,6),'bark_dark')
 return m

def make_rock(name,width,depth,height,seed,plateau=False,cluster=False):
 m=Model(name,'rocks','Roche facettée asymétrique, matériau mat')
 if cluster:
  for j,ofs in enumerate([(0,0),(.65,.15),(-.40,.32)]):
   w=width*(.58 if j else .80)
   d=depth*(.72 if j else .90)
   h=height*(.6 if j else 1.)
   rock_parts(m,irregular_rock(w,d,h,seed+j*37,False),base='rock_mid' if j<2 else 'rock_warm',salt=j+seed%40)
   part_n=len(m.parts)
   for i in range(part_n-3,part_n):
    if i>=0:
     part,mat=m.parts[i]
     part.apply_translation([ofs[0],ofs[1],0])
 else:
  rock_parts(m,irregular_rock(width,depth,height,seed,plateau),base='rock_mid' if 'boulder' in name else 'rock_warm',salt=seed%40)
 return m

models=[
 make_broadleaf('tree_oak_round',6.2,3.15,'round',101),
 make_broadleaf('tree_oak_wide',5.4,3.65,'wide',102),
 make_broadleaf('tree_oak_tall',8.0,2.70,'tall',103),
 make_broadleaf('tree_young',3.3,1.65,'round',104),
 make_pine('tree_pine_small',4.4,2.45,201,3),
 make_pine('tree_pine_medium',6.4,3.0,202,4),
 make_pine('tree_pine_tall',8.5,3.45,203,4),
 make_rock('rock_boulder_small',1.35,1.1,.78,301),
 make_rock('rock_boulder_medium',2.35,1.8,1.55,302),
 make_rock('rock_boulder_large',3.4,2.55,2.1,303),
 make_rock('rock_slab_flat',2.7,1.9,.65,304,True),
 make_rock('rock_cluster',2.5,2.2,1.4,305,cluster=True),
 make_bush('bush_low',1.25,.55,401,3),
 make_bush('bush_round',1.65,1.08,402,5),
 make_bush('bush_wild',2.1,1.10,403,7),
 make_grass('grass_short',6,.48,501),
 make_grass('grass_tall',9,.92,502),
 make_grass('grass_wild',13,.80,503),
 make_fern('fern_small',5,.65,601),
 make_fern('fern_large',7,1.12,602),
 make_log('log_fallen_small',1.75,.23,701),
 make_log('log_fallen_large',3.0,.37,702),
 make_log('stump_old',.71,.35,703,True),
]

catalog=[m.save() for m in models]
(ROOT/'catalog.json').write_text(json.dumps({'pack':'Motor-Trail Nature Prototype','version':'0.1','seed':SEED,'colors':PALETTE,'assets':catalog},indent=2,ensure_ascii=False),encoding='utf-8')

# Rasterize actual mesh triangles into an orthographic preview, preserving material colors.
TILE_W,TILE_H=340,320
cols=5
rows=math.ceil(len(models)/cols)
img=Image.new('RGB',(cols*TILE_W,rows*TILE_H),(237,240,235))
draw=ImageDraw.Draw(img)
try:font=ImageFont.truetype('/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf',16)
except:font=ImageFont.load_default()
try:bigfont=ImageFont.truetype('/usr/share/fonts/truetype/dejavu/DejaVuSans-Bold.ttf',16)
except:bigfont=font

eye=np.array([7.,-10.,8.]);eye/=np.linalg.norm(eye)
right=np.cross(np.array([0.,0.,1.]),eye);right/=np.linalg.norm(right)
up=np.cross(eye,right);up/=np.linalg.norm(up)
light=np.array([-0.50,-.35,.81]);light/=np.linalg.norm(light)

for idx,m in enumerate(models):
 col=idx%cols;row=idx//cols; ox=col*TILE_W;oy=row*TILE_H
 draw.rounded_rectangle((ox+9,oy+8,ox+TILE_W-9,oy+TILE_H-8),radius=11,fill=(248,249,246),outline=(219,226,220),width=2)
 bounds=m.bounds();minv,maxv=bounds
 center=np.array([ (minv[0]+maxv[0])/2,(minv[1]+maxv[1])/2,(minv[2]+maxv[2])/2 ])
 # Project all vertices into right/up axes to determine fit independently.
 vv=np.concatenate([a.vertices for a,_ in m.parts],axis=0)
 projected=np.stack([(vv-center)@right,(vv-center)@up],axis=-1)
 size=projected.max(axis=0)-projected.min(axis=0)
 scale=min(270/max(size[0],.8), 241/max(size[1],.8), 95 if m.category=='plants' else 145)
 mid2d=(projected.min(axis=0)+projected.max(axis=0))/2
 px=ox+TILE_W/2;py=oy+TILE_H/2-16
 # Delicate neutral shadow at ground level.
 shadow_y=oy+TILE_H-55 if m.category in ('trees','rocks','bushes') else oy+TILE_H-63
 draw.ellipse((px-45,shadow_y-9,px+45,shadow_y+6),fill=(227,232,225))
 tris=[]
 for mesh,material in m.parts:
  vertices=mesh.vertices;faces=mesh.faces
  rgb=rgba(PALETTE[material])[:3]
  normals=mesh.face_normals
  for fi,f in enumerate(faces):
   tri=vertices[f]
   v=(tri-center)
   xy=np.stack([v@right,v@up],axis=-1)
   xy=(xy-mid2d)*scale
   pts=[(float(px+p[0]),float(py-p[1])) for p in xy]
   depth=float(np.mean(v@eye))
   ndot=max(0.,float(np.dot(normals[fi],light)))
   lighting=.82+.15*ndot
   colrgb=tuple(int(max(0,min(255,k*lighting))) for k in rgb)
   tris.append((depth,pts,colrgb))
 # Depth test using painter's algorithm. farther first
 tris.sort(key=lambda t:t[0])
 for depth,pts,color in tris:draw.polygon(pts,fill=color)
 draw.text((ox+24,oy+TILE_H-37),m.name.replace('_',' '),fill=(58,73,63),font=bigfont)
 draw.text((ox+24,oy+24),m.category.upper(),fill=(126,141,131),font=font)
img.save(ROOT/'preview.png')

print(f'Created {len(catalog)} GLB models, {sum(x["triangles"] for x in catalog)} triangles total')
print(f'Preview: {ROOT/"preview.png"}')
