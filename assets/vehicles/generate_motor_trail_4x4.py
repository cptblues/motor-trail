"""Generate a modular low-poly 4x4 asset, with named wheel pivots and glTF demo animations.
Dependencies: numpy, trimesh, Pillow (only for preview). No Blender necessary.
Coordinate system: +Y up, vehicle forward -Z, width along X. All dimensions metres.
"""
import json, math, struct
from collections import defaultdict
from pathlib import Path
import numpy as np
import trimesh
from trimesh.transformations import rotation_matrix

OUT = Path(__file__).parent

class Vehicle:
    def __init__(self):
        self.nodes=[]; self.node_names={}; self.geo=defaultdict(list)
        self.materials=[]; self.mats={}; self.positions={}
        self.root=self.node('MotorTrail_4x4')

    def material(self, name, color, metallic=0.0, rough=0.8):
        r,g,b=color
        idx=len(self.materials)
        self.materials.append({'name':name,'pbrMetallicRoughness':{'baseColorFactor':[r/255,g/255,b/255,1.0], 'metallicFactor':float(metallic),'roughnessFactor':float(rough)},'doubleSided':True})
        self.mats[name]=idx
        return name

    def node(self,name,parent=None,translation=None,rotation=None):
        d={'name':name}
        if translation is not None: d['translation']=[float(v) for v in translation]
        if rotation is not None: d['rotation']=[float(v) for v in rotation]
        idx=len(self.nodes); self.nodes.append(d)
        self.node_names[name]=idx
        if parent is not None: self.nodes[parent].setdefault('children',[]).append(idx)
        return idx

    def add(self, parent, mesh, mat):
        self.geo[(parent,self.mats[mat])].append(mesh)

    def box(self, n, center, size, mat, rot=None):
        m=trimesh.creation.box(extents=size)
        if rot is not None:
            if np.isscalar(rot):
                R=rotation_matrix(rot,[1,0,0]); m.apply_transform(R)
            else: m.apply_transform(rot)
        m.apply_translation(center)
        self.add(n,m,mat)

    def bar(self,n,a,b,r,mat,sides=10):
        a=np.asarray(a,float); b=np.asarray(b,float); delta=b-a
        if np.linalg.norm(delta)<1e-7: return
        m=trimesh.creation.cylinder(radius=r,height=float(np.linalg.norm(delta)), sections=sides)
        m.apply_transform(trimesh.geometry.align_vectors([0,0,1],delta))
        m.apply_translation((a+b)/2)
        self.add(n,m,mat)

    def cyl(self,n,center,r,h,mat,axis='Z',sections=16):
        m=trimesh.creation.cylinder(radius=r,height=h,sections=sections)
        if axis=='X': m.apply_transform(rotation_matrix(math.pi/2,[0,1,0]))
        elif axis=='Y': m.apply_transform(rotation_matrix(math.pi/2,[1,0,0]))
        m.apply_translation(center)
        self.add(n,m,mat)

    def prism_yz(self,n,poly,xmin,xmax,mat):
        pts0=[[xmin,y,z] for z,y in poly]
        pts1=[[xmax,y,z] for z,y in poly]
        v=np.array(pts0+pts1,float); L=len(poly)
        faces=[]
        for k in range(1,L-1): faces.extend([[0,k+1,k],[L,L+k,L+k+1]])
        for k in range(L):
            j=(k+1)%L
            faces.extend([[k,j,L+j],[k,L+j,L+k]])
        self.add(n,trimesh.Trimesh(vertices=v,faces=faces,process=False),mat)

    def quad(self,n,p,mat):
        v=np.array(p,dtype=float)
        f=np.array([[0,1,2],[0,2,3],[0,2,1],[0,3,2]])
        self.add(n,trimesh.Trimesh(vertices=v,faces=f,process=False),mat)

    def ring(self,n,center,axis,r_outer,r_inner,width,mat,segments=20):
        # Disk with hollow center, double-side and inner/outer edges.
        axis_vector=np.array({'X':[1,0,0],'Y':[0,1,0],'Z':[0,0,1]}[axis],float)
        tmp=[]
        for pos,r in [(-width/2,r_inner),(-width/2,r_outer),(width/2,r_outer),(width/2,r_inner)]:
            for i in range(segments):
                a=2*math.pi*i/segments
                tmp.append([pos,r*math.sin(a),r*math.cos(a)])
        vs=np.asarray(tmp,float); fs=[]
        for j in range(4):
            k=(j+1)%4
            for i in range(segments):
                q=(i+1)%segments
                a=j*segments+i; b=j*segments+q; c=k*segments+q; d=k*segments+i
                fs.extend([[a,b,c],[a,c,d]])
        mesh=trimesh.Trimesh(vertices=vs,faces=fs,process=False)
        if axis=='Z': mesh.apply_transform(rotation_matrix(-math.pi/2,[0,1,0]))
        if axis=='Y': mesh.apply_transform(rotation_matrix(math.pi/2,[0,0,1]))
        mesh.apply_translation(center); self.add(n,mesh,mat)

    def wheel(self, n, x_side=1, spare=False):
        # The wheel geometry lives at the origin of a spin pivot. Rim, tire, lugs are independently materialized.
        wheel_profile=[(-.171,.320),(-.171,.395),(-.158,.475),(-.118,.517),(-.078,.530),(.078,.530),(.118,.517),(.158,.475),(.171,.395),(.171,.320)]
        angles=24
        verts=[]; faces=[]
        for x,rad in wheel_profile:
            for i in range(angles):
                a=2*math.pi*i/angles
                verts.append([x,rad*math.sin(a),rad*math.cos(a)])
        L=len(wheel_profile)
        for j in range(L):
            nxt=(j+1)%L
            for i in range(angles):
                a=j*angles+i; b=j*angles+(i+1)%angles; c=nxt*angles+(i+1)%angles; d=nxt*angles+i
                faces.extend([[a,b,c],[a,c,d]])
        self.add(n,trimesh.Trimesh(vertices=verts,faces=faces,process=False),'Tire rubber')
        # Three bands of raised tread blocks, staggered.
        for band,xx in enumerate((-.106,0,.106)):
            for i in range(18):
                phi=2*math.pi*(i+(.45 if band%2 else 0))/18
                # The long tread dimension follows the tangent to the wheel circumference.
                m=trimesh.creation.box(extents=(.090,.143,.035))
                m.apply_transform(rotation_matrix(-phi,[1,0,0]))
                m.apply_translation([xx,.533*math.sin(phi),.533*math.cos(phi)])
                self.add(n,m,'Tread')
        # Rim side rings / dark internal hollow.
        self.cyl(n,(0,0,0),.325,.29,'Inner wheel',axis='X',sections=24)
        for side in (-1,1):
            self.ring(n,(side*.169,0,0),'X',.315,.226,.037,'Wheel rim')
            self.ring(n,(side*.191,0,0),'X',.220,.185,.018,'Dark alloy')
            self.cyl(n,(side*.200,0,0),.117,.025,'Hub',axis='X',sections=12)
            for k in range(6):
                a=k*math.tau/6
                m=trimesh.creation.box(extents=(.034,.090,.187))
                m.apply_transform(rotation_matrix(-a,[1,0,0]))
                m.apply_translation((side*.200,math.sin(a)*.16,math.cos(a)*.16))
                self.add(n,m,'Wheel rim')
                aa=a+math.pi/6
                self.cyl(n,(side*.222,.079*math.sin(aa),.079*math.cos(aa)),.017,.02,'Bolts',axis='X',sections=8)
            self.cyl(n,(side*.229,0,0),.058,.02,'Hub cap',axis='X',sections=12)

    def build(self):
        for name,color,met,rough in [
            ('Olive body',(110,126,76),.08,.72),('Olive light',(138,149,95),.06,.78),
            ('Olive dark',(69,82,55),.07,.83),('Black trim',(31,38,36),.05,.81),
            ('Steel dark',(43,48,48),.50,.62),('Steel frame',(58,62,59),.64,.55),
            ('Steel light',(123,128,123),.68,.43),('Glass',(38,62,72),.10,.20),
            ('Glass shine',(78,104,110),.16,.24),('Lights',(244,235,203),.02,.20),
            ('Red lights',(181,45,33),.05,.40),('Amber',(238,141,50),.04,.4),
            ('Hook orange',(215,81,35),.27,.44),('Tire rubber',(35,37,33),.0,.97),
            ('Tread',(49,49,42),.0,.99),('Inner wheel',(25,29,28),.2,.82),
            ('Wheel rim',(114,120,113),.75,.42),('Dark alloy',(49,55,52),.65,.55),
            ('Hub',(76,81,76),.75,.43),('Hub cap',(153,154,134),.69,.36),
            ('Bolts',(180,176,156),.82,.25),('Cargo green',(86,105,69),.07,.89),
            ('Cargo rust',(173,63,36),.08,.74),('Seats',(51,57,52),.02,.89)
        ]: self.material(name,color,met,rough)

        ch=self.node('Chassis',self.root)
        body=self.node('Body',self.root)
        glass=self.node('Windows',body)
        cosmetics=self.node('Details',body)
        bumper=self.node('Bumpers_Recovery',self.root)
        acc=self.node('Roof_Rack_Equipment',body)
        susp_parts=self.node('Suspension_Visuals',ch)

        # Ladder-frame chassis with steel beams, cross members, underside and skid plate.
        for side in (-1,1):
            x=side*.55
            self.box(ch,(x,.84,0),(.105,.155,3.91),'Steel frame')
        for z in (-1.70,-.75,.2,1.18,1.7):
            self.box(ch,(0,.85,z),(1.2,.12,.095),'Steel frame')
        self.box(ch,(0,.976,.08),(1.48,.080,2.75),'Steel dark')
        self.box(ch,(0,.708,-1.49),(1.20,.053,.55),'Steel light')
        for x in (-.43,-.14,.14,.43):
            self.box(ch,(x,.674,-1.48),(.09,.022,.34),'Steel dark')
        self.cyl(ch,(0,.775,-.08),.235,.30,'Steel frame',axis='Y',sections=12)
        self.cyl(ch,(0,.775,1.22),.22,.27,'Steel frame',axis='Y',sections=12)
        for z in (-1.27,1.27):
            self.bar(ch,(-.85,.64,z),(.85,.64,z),.066,'Steel frame')
            for x in (-.78,.78): self.bar(ch,(x,.65,z),(x*.76,.94,z),.045,'Steel frame')

        # Body tub, hood and roof: silhouette crafted as a few broad low-poly extruded shells.
        self.prism_yz(body,[(-1.83,.97),(-1.84,1.47),(-1.64,1.60),(-.56,1.62),(-.49,1.10),(-.59,.99)],-.83,.83,'Olive body')
        self.prism_yz(body,[(-.67,.97),(-.67,1.54),(1.58,1.54),(1.79,1.42),(1.80,.96)],-.83,.83,'Olive body')
        self.prism_yz(body,[(-.72,1.50),(-.47,2.125),(1.32,2.125),(1.63,1.52)],-.755,.755,'Olive body')
        self.box(body,(0,2.134,.42),(1.57,.060,1.90),'Olive light')
        self.box(body,(0,1.615,-1.19),(1.48,.027,1.21),'Olive light')
        self.box(body,(0,.996,.20),(1.74,.064,2.73),'Olive dark')
        # Hood slight ridge.
        self.box(body,(0,1.636,-1.17),(.34,.023,1.0),'Olive body')
        # All windows are real separate polygons with dark glass material.
        self.quad(glass,[(-.690,1.645,-.728),(.690,1.645,-.728),(.686,2.067,-.474),(-.686,2.067,-.474)],'Glass')
        self.bar(glass,(-.70,1.66,-.749),(.70,1.66,-.749),.023,'Black trim')
        self.bar(glass,(-.70,2.071,-.474),(.70,2.071,-.474),.022,'Black trim')
        self.quad(glass,[(-.681,1.639,1.637),(.681,1.639,1.637),(.687,2.064,1.338),(-.687,2.064,1.338)],'Glass')
        for side in (-1,1):
            x=side*.768
            # Windows front cabin with angled windshield A-pillar, middle post and rear pane.
            self.quad(glass,[(x,1.666,-.455),(x,2.056,-.386),(x,2.056,.442),(x,1.678,.443)],'Glass')
            self.quad(glass,[(x,1.677,.563),(x,2.055,.563),(x,2.055,1.295),(x,1.687,1.487)],'Glass')
            # Pillars and decorative seals.
            for z in (.498,): self.bar(body,(x,1.586,z),(x,2.106,z),.041,'Olive dark')
            self.bar(body,(x,1.65,-.492),(x,2.12,-.440),.043,'Olive light')
            self.bar(body,(x,1.62,1.570),(x,2.12,1.34),.048,'Olive light')
            self.bar(body,(x,1.645,-.443),(x,1.645,1.505),.026,'Black trim')
            # Doors, door seams, subtle raised side panel strips.
            self.box(cosmetics,(side*.843,1.272,-.015),(.021,.455,1.02),'Olive light')
            self.bar(cosmetics,(side*.863,1.01,.510),(side*.863,1.65,.510),.011,'Olive dark')
            self.box(cosmetics,(side*.858,1.497,.236),(.042,.048,.17),'Black trim')
            self.box(cosmetics,(side*.868,1.491,.234),(.047,.015,.080),'Steel light')
            self.box(cosmetics,(side*.82,1.055,.27),(.045,.034,2.46),'Black trim')
            self.box(cosmetics,(side*.883,.901,.19),(.21,.062,1.81),'Steel frame')
            # Body side indicator, mirrors, hinges.
            self.box(cosmetics,(side*.864,1.502,-1.63),(.042,.09,.13),'Amber')
            self.box(cosmetics,(side*1.009,1.766,-.47),(.218,.18,.23),'Black trim')
            self.box(cosmetics,(side*1.13,1.77,-.47),(.020,.114,.15),'Glass')
            self.bar(cosmetics,(side*.80,1.72,-.47),(side*1.07,1.71,-.47),.029,'Steel dark')
            # Upper fender arches, 9 low-poly overlapping radial segments with depth.
            for zc in (-1.26,1.27):
                for i in range(10):
                    a=math.pi*i/10; b=math.pi*(i+1)/10
                    # Radial upper arc in side Y/Z plane.
                    p=[(side*.88,.530+.638*math.sin(a),zc+.638*math.cos(a)),
                       (side*.88,.530+.638*math.sin(b),zc+.638*math.cos(b)),
                       (side*.88,.530+.545*math.sin(b),zc+.545*math.cos(b)),
                       (side*.88,.530+.545*math.sin(a),zc+.545*math.cos(a))]
                    self.quad(cosmetics,p,'Black trim')
                    self.bar(cosmetics,(side*.920,p[0][1],p[0][2]),(side*.920,p[1][1],p[1][2]),.035,'Black trim')
            # small bolts on arch corners
            for zc in (-1.26,1.27):
                for a in (.22,.82,1.57,2.32,2.92):
                    self.cyl(cosmetics,(side*.929,.530+.615*math.sin(a),zc+.615*math.cos(a)),.015,.018,'Bolts',axis='X',sections=8)
            # rear wing and tail indicator
            self.box(cosmetics,(side*.848,1.39,1.745),(.055,.28,.045),'Red lights')

        # Front grille and lamps, trim and headlights on the nose.
        self.box(cosmetics,(0,1.382,-1.858),(1.39,.32,.037),'Black trim')
        for x in np.linspace(-.40,.40,7):
            self.box(cosmetics,(float(x),1.382,-1.897),(.060,.235,.025),'Steel dark')
        for side in (-1,1):
            x=side*.645
            self.cyl(cosmetics,(x,1.415,-1.910),.161,.047,'Steel light',axis='Z',sections=20)
            self.cyl(cosmetics,(x,1.415,-1.940),.127,.022,'Lights',axis='Z',sections=20)
            self.cyl(cosmetics,(x,1.415,-1.953),.076,.008,'Glass shine',axis='Z',sections=16)
            self.box(cosmetics,(side*.755,1.557,-1.857),(.104,.102,.051),'Amber')
        self.box(cosmetics,(0,1.654,-1.77),(1.26,.025,.17),'Olive dark')
        self.box(cosmetics,(0,1.07,1.808),(1.56,.20,.065),'Olive dark')
        for side in (-1,1):
            self.box(cosmetics,(side*.61,1.329,1.832),(.26,.225,.042),'Red lights')
            self.box(cosmetics,(side*.61,1.432,1.856),(.22,.06,.018),'Amber')
        self.box(cosmetics,(0,1.11,1.85),(.41,.18,.04),'Black trim')

        # Steel bumpers, bars, winch, shackles, license plates.
        for z in (-2.007,1.960):
            self.box(bumper,(0,1.013,z),(1.96,.155,.21),'Steel dark')
            for side in (-1,1):
                self.box(bumper,(side*.89,1.01,z),(.245,.185,.26),'Black trim')
        self.box(bumper,(0,.913,-2.143),(.575,.19,.185),'Steel frame')
        self.cyl(bumper,(0,.914,-2.252),.092,.47,'Steel light',axis='X',sections=16)
        for side in (-1,1):
            x=side*.77
            self.bar(bumper,(x,1.10,-2.03),(x,1.47,-2.04),.040,'Steel frame')
            self.bar(bumper,(x,1.47,-2.04),(side*.50,1.58,-2.025),.039,'Steel frame')
        self.bar(bumper,(-.77,1.47,-2.04),(.77,1.47,-2.04),.041,'Steel frame')
        for side in (-1,1):
            # Recovery D shackles from 3 bars with orange/rust tones.
            x=side*.50; z=-2.183
            self.bar(bumper,(x-.05,.947,z),(x-.05,.82,z),.028,'Hook orange')
            self.bar(bumper,(x+.05,.947,z),(x+.05,.82,z),.028,'Hook orange')
            self.bar(bumper,(x-.05,.82,z),(x+.05,.82,z),.034,'Hook orange')
            self.cyl(bumper,(x,.95,z),.055,.11,'Steel light',axis='X',sections=10)
        self.box(bumper,(0,.962,2.095),(.50,.08,.115),'Steel frame')

        # Roof rack base and tubular rails; real parts, not a decal.
        self.box(acc,(0,2.240,.42),(1.55,.045,1.79),'Black trim')
        for side in (-1,1):
            xx=side*.735
            for z in (-.30,.26,.82,1.26):
                self.bar(acc,(xx,2.205,z),(xx,2.375,z),.022,'Steel frame')
            self.bar(acc,(xx,2.378,-.48),(xx,2.378,1.36),.027,'Steel dark')
            self.bar(acc,(xx,2.235,-.49),(xx,2.385,-.49),.026,'Steel frame')
            self.bar(acc,(xx,2.235,1.36),(xx,2.385,1.36),.026,'Steel frame')
        self.bar(acc,(-.74,2.38,-.49),(.74,2.38,-.49),.023,'Steel dark')
        self.bar(acc,(-.74,2.38,1.36),(.74,2.38,1.36),.023,'Steel dark')
        for z in (-.33,.07,.47,.87,1.26): self.bar(acc,(-.70,2.25,z),(.70,2.25,z),.018,'Steel frame')
        # Roof cargo / boxes + jerry can + recovery boards.
        for x,z,sx,sz in [(-.36,.57,.48,.39),(-.31,1.00,.53,.29),(.22,.62,.42,.35)]:
            self.box(acc,(x,2.327,z),(sx,.135,sz),'Cargo green')
            self.box(acc,(x,2.407,z),(sx*.94,.023,sz*.92),'Olive dark')
            for dx in (-sx*.35,sx*.35): self.box(acc,(x+dx,2.345,z),(.023,.21,sz*.90),'Steel dark')
        self.box(acc,(.43,2.365,1.05),(.28,.25,.28),'Cargo rust')
        self.box(acc,(.43,2.505,1.05),(.14,.035,.16),'Black trim')
        for z in (.05,.18): self.box(acc,(.36,2.297,z),(.58,.038,.083),'Hook orange')
        for side in (-1,1):
            self.box(acc,(side*.58,2.21,-.12),(.13,.085,.21),'Black trim')
            self.box(acc,(side*.58,2.23,-.12),(.088,.052,.14),'Lights')
        # Snorkel pipe on passenger side.
        self.bar(cosmetics,(-.84,1.55,-.62),(-.99,1.84,-.62),.053,'Black trim')
        self.bar(cosmetics,(-.99,1.84,-.62),(-.99,2.19,-.62),.053,'Black trim')
        self.box(cosmetics,(-.99,2.220,-.64),(.135,.095,.18),'Black trim')
        # Spare tire on tailgate (the assembly remains removable as its own node).
        spare=self.node('Spare_Tire_Detachable',body,translation=(0,1.39,2.07),rotation=(0,-math.sin(math.pi/4),0,math.cos(math.pi/4)))
        self.wheel(spare,spare=True)
        self.bar(cosmetics,(-.26,1.4,1.86),(.26,1.4,1.86),.046,'Steel dark')

        # Axles, shock and spring visual geometry, per-wheel suspension anchored to the named visual nodes.
        for axlename,z in [('Front',-1.26),('Rear',1.27)]:
            self.bar(susp_parts,(-.89,.63,z),(.89,.63,z),.060,'Steel frame')
            for s in (-1,1):
                x=s*.805
                self.bar(susp_parts,(x,1.12,z+.11),(x,0.70,z+.11),.040,'Steel light')
                # Orange coil spring turns, faceted and openly visible.
                for j in range(6):
                    y=.77+j*.055
                    self.ring(susp_parts,(x,y,z+.11),'Y',.100,.080,.024,'Hook orange',segments=12)
                self.cyl(susp_parts,(x,.76,z+.11),.050,.08,'Steel frame',axis='Y',sections=10)
                self.bar(susp_parts,(s*.50,.75,z),(s*.93,.59,z),.045,'Steel frame')
                self.bar(susp_parts,(s*.53,1.00,z+.15),(s*.94,.66,z+.02),.033,'Steel frame')
        self.bar(susp_parts,(-.88,.65,-1.15),(.88,.65,-1.15),.035,'Steel light')

        # Four independent, movable suspension/steering/rolling node chains.
        self.anim_groups={'suspensions':[], 'steering':[], 'spin':[]}
        for label,x,z in [('FL',-.98,-1.26),('FR',.98,-1.26),('RL',-.98,1.27),('RR',.98,1.27)]:
            sn=self.node('Suspension_'+label,self.root,translation=(x,.56,z))
            self.anim_groups['suspensions'].append(sn)
            if label.startswith('F'):
                steer=self.node('Steer_'+label,sn)
                self.anim_groups['steering'].append(steer)
                parent=steer
            else: parent=sn
            spin=self.node('WheelSpin_'+label,parent)
            self.anim_groups['spin'].append(spin)
            self.wheel(spin,x_side=1 if x>0 else -1)
        return self

    def export(self,path):
        binary=bytearray(); buffer_views=[]; accessors=[]; meshes=[]
        def data_block(raw,target=None,alignment=4):
            while len(binary)%alignment: binary.append(0)
            offset=len(binary); binary.extend(raw)
            v={'buffer':0,'byteOffset':offset,'byteLength':len(raw)}
            if target: v['target']=target
            buffer_views.append(v)
            return len(buffer_views)-1
        def accessor(array,component_type,typ,target=None):
            a=np.ascontiguousarray(array)
            bv=data_block(a.tobytes(),target)
            d={'bufferView':bv,'componentType':component_type,'count':len(a),'type':typ}
            if typ=='VEC3' and component_type==5126:
                d['min']=np.min(a,axis=0).astype(float).tolist(); d['max']=np.max(a,axis=0).astype(float).tolist()
            accessors.append(d); return len(accessors)-1
        bynode=defaultdict(list)
        for (node,mat),parts in self.geo.items():
            # Merge all pieces of the same material into one primitive on its logical glTF node.
            mesh=trimesh.util.concatenate(parts)
            # Expand triangle corner vertices for faceted, hard-edged game-asset shading.
            tri=mesh.triangles.astype(np.float32)
            normals=mesh.face_normals.astype(np.float32)
            positions=tri.reshape(-1,3)
            normal=np.repeat(normals,3,axis=0)
            if len(positions)==0: continue
            indices=np.arange(len(positions),dtype=np.uint32)
            if np.max(indices)<65536: indices=indices.astype(np.uint16); comp=5123
            else: comp=5125
            prim={'attributes':{'POSITION':accessor(positions,5126,'VEC3',34962),'NORMAL':accessor(normal,5126,'VEC3',34962)},
                  'indices':accessor(indices,comp,'SCALAR',34963),'material':mat,'mode':4}
            bynode[node].append(prim)
        for node,prims in bynode.items():
            self.nodes[node]['mesh']=len(meshes)
            meshes.append({'name':self.nodes[node]['name']+'_Geometry','primitives':prims})

        animations=[]
        def animation(name, times, data):
            ta=accessor(np.array(times,dtype=np.float32),5126,'SCALAR')
            channels=[]; samplers=[]
            for idx,path,values in data:
                dtype=np.float32; v=np.asarray(values,dtype=dtype)
                out_acc=accessor(v,5126,'VEC4' if path=='rotation' else 'VEC3')
                si=len(samplers); samplers.append({'input':ta,'output':out_acc,'interpolation':'LINEAR'})
                channels.append({'sampler':si,'target':{'node':idx,'path':path}})
            animations.append({'name':name,'channels':channels,'samplers':samplers})
        def qy(a): return [0,math.sin(a/2),0,math.cos(a/2)]
        def qx(a): return [math.sin(a/2),0,0,math.cos(a/2)]
        animation('DEMO_Steering_Left_Right',[0,.9,1.8,2.7,3.6],
            [(n,'rotation',[qy(a) for a in [0,-.49,0,.49,0]]) for n in self.anim_groups['steering']])
        animation('DEMO_Independent_Suspension',[0,.8,1.6,2.4,3.2],
            [(n,'translation',[[float(self.nodes[n]['translation'][0]),float(self.nodes[n]['translation'][1])+float(d),float(self.nodes[n]['translation'][2])]
                 for d in seq]) for n,seq in zip(self.anim_groups['suspensions'],
                 [[0,.15,-.05,0,0],[0,-.09,.11,0,0],[0,-.06,.14,0,0],[0,.12,-.11,0,0]])])
        animation('DEMO_Rolling_Wheels',[0,.5,1,1.5,2],
                  [(n,'rotation',[qx(a) for a in [0,math.pi/2,math.pi,math.pi*1.5,math.tau]])
                   for n in self.anim_groups['spin']])
        obj={'asset':{'version':'2.0','generator':'MotorTrail procedural 4x4 generator'},
             'scene':0,'scenes':[{'name':'MotorTrail_4x4','nodes':[self.root]}],
             'nodes':self.nodes,'meshes':meshes,'materials':self.materials,
             'accessors':accessors,'bufferViews':buffer_views,
             'buffers':[{'byteLength':len(binary)}],'animations':animations}
        while len(binary)%4: binary.append(0)
        jsonraw=json.dumps(obj,separators=(',',':'),ensure_ascii=False).encode('utf8')
        while len(jsonraw)%4: jsonraw+=b' '
        full=12+8+len(jsonraw)+8+len(binary)
        with open(path,'wb') as f:
            f.write(struct.pack('<4sII',b'glTF',2,full))
            f.write(struct.pack('<I4s',len(jsonraw),b'JSON')); f.write(jsonraw)
            f.write(struct.pack('<I4s',len(binary),b'BIN\0')); f.write(binary)
        return obj

if __name__=='__main__':
    v=Vehicle().build()
    path=OUT/'motor_trail_4x4.glb'
    o=v.export(path)
    print('Wrote',path,'bytes=',path.stat().st_size)
    print('Nodes',len(o['nodes']),'meshes',len(o['meshes']),'animations',list(a['name'] for a in o['animations']))
    print('Triangles',sum(len(m.faces) for sub in v.geo.values() for m in sub))
