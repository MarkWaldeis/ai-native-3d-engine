"""Deterministic jungle FPS kit. Run with Blender --background --python ... -- OUT."""
import json
import math
from pathlib import Path
import random
import sys

import bpy
from mathutils import Vector


ROOT = Path(__file__).resolve().parents[2]
OUT = Path(sys.argv[sys.argv.index('--') + 1]).resolve()
OUT.mkdir(parents=True, exist_ok=True)
random.seed(715)
bpy.ops.object.select_all(action='SELECT')
bpy.ops.object.delete(use_global=False)


def material(name, color, roughness=0.85, metal=0):
    m = bpy.data.materials.new(name)
    m.diffuse_color = (*color, 1)
    m.use_nodes = True
    p = m.node_tree.nodes.get('Principled BSDF')
    p.inputs['Base Color'].default_value = (*color, 1)
    p.inputs['Roughness'].default_value = roughness
    p.inputs['Metallic'].default_value = metal
    return m


M = {n: material(n, c, r, me) for n, c, r, me in [
    ('bark', (.21,.12,.065), .95,0), ('wood', (.35,.25,.13), .92,0),
    ('leaf', (.075,.24,.075), .85,0), ('leaf_light', (.22,.39,.09), .85,0),
    ('leaf_dark', (.035,.12,.055), .95,0), ('stone', (.33,.37,.32), .95,0),
    ('concrete', (.49,.48,.39), .98,0), ('sand', (.48,.43,.26), .99,0),
    ('olive', (.19,.24,.115), .85,0), ('camo', (.33,.36,.18), .9,0),
    ('metal', (.12,.15,.15), .43,.72), ('rust', (.30,.13,.065), .9,.4),
    ('black', (.025,.033,.035), .65,.15), ('skin', (.47,.31,.19), .85,0),
    ('white', (.78,.79,.70), .65,0), ('red', (.57,.035,.025), .65,0),
    ('glass', (.055,.24,.28), .18,.55), ('blue', (.055,.16,.27), .85,0),
]}
parts = []


def finish(o, mat):
    o.data.materials.append(M[mat])
    parts.append(o)
    return o


def box(p, s, mat, bevel=0.025):
    bpy.ops.mesh.primitive_cube_add(size=1, location=p)
    o = bpy.context.object
    o.scale = s
    bpy.ops.object.transform_apply(location=False, rotation=False, scale=True)
    if bevel:
        mod = o.modifiers.new('edge wear', 'BEVEL')
        mod.width = min(bevel, min(s)*.18)
        mod.segments = 1
        bpy.ops.object.modifier_apply(modifier=mod.name)
    return finish(o, mat)


def ball(p, s, mat, sub=2, irregular=0):
    bpy.ops.mesh.primitive_ico_sphere_add(subdivisions=sub, radius=1, location=p)
    o = bpy.context.object
    for v in o.data.vertices:
        v.co *= 1 + random.uniform(-irregular, irregular)
    o.scale = s
    bpy.ops.object.transform_apply(location=False, rotation=False, scale=True)
    return finish(o, mat)


def rod(a, b, radius, mat, vertices=10, top=None):
    a, b = Vector(a), Vector(b)
    bpy.ops.mesh.primitive_cone_add(vertices=vertices, radius1=radius,
        radius2=radius if top is None else top, depth=(b-a).length, location=(a+b)/2)
    o = bpy.context.object
    o.rotation_euler = (b-a).to_track_quat('Z','Y').to_euler()
    return finish(o, mat)


def mesh(verts, faces, mat):
    data = bpy.data.meshes.new(mat)
    data.from_pydata(verts, [], faces)
    data.update()
    o = bpy.data.objects.new(mat, data)
    bpy.context.collection.objects.link(o)
    return finish(o, mat)


def frond(base, angle, length, width, rise=0):
    # Folded double-sided leaflets along an arched central rib.
    base = Vector(base)
    direction = Vector((math.cos(angle),math.sin(angle),0))
    side = Vector((-math.sin(angle),math.cos(angle),0))
    points = [base + direction*length*t + Vector((0,0,math.sin(t*math.pi)*length*.23 + rise*t))
              for t in [i/8 for i in range(9)]]
    for i in range(8):
        rod(points[i],points[i+1],.018,'leaf_light',5)
        for sign in [-1,1]:
            p = points[i]
            tip = p + side*sign*width*math.sin((i+1)/9*math.pi) + direction*length*.16
            mesh([p, points[i+1], tip+Vector((0,0,.035)), tip], [(0,1,2),(0,2,3)],
                 'leaf' if i%2 else 'leaf_light')


def tree(kind):
    h = {'tree_palm':10,'tree_broadleaf':14,'tree_kapok':24}[kind]
    if kind == 'tree_palm':
        for i in range(16):
            z = i*h/16
            rod((.5*(z/h)**2,0,z),(.5*((z+h/16)/h)**2,0,z+h/16),
                .24*(1-i/26),'bark',12, .24*(1-(i+1)/26))
            rod((.5*(z/h)**2,0,z),(.5*(z/h)**2,0,z+.06),.245*(1-i/26),'wood',12)
        for i in range(11):
            frond((.5,0,h-.5),i*math.tau/11,3.3,.68, -.6 if i%2 else .1)
        for i in range(5):
            ball((.5+random.uniform(-.3,.3),random.uniform(-.3,.3),h-.6),(.16,.16,.20),'bark')
    else:
        r = .65 if h==14 else 1.2
        rod((0,0,0),(0,0,h*.76),r,'bark',14,r*.4)
        for i in range(7):
            a=i*math.tau/7
            x,y=math.cos(a),math.sin(a)
            mesh([(x*r,y*r,0),(x*r*3,y*r*3,0),(x*r,y*r,h*.22),
                  (x*r-y*.3,y*r+x*.3,0)],[(0,1,2),(1,3,2),(3,0,2),(0,3,1)],'bark')
            end=(x*h*.24,y*h*.24,h*.79)
            rod((0,0,h*.5),end,r*.3,'bark',8,.1)
            ball(end,(h*.22,h*.21,h*.16),['leaf','leaf_light','leaf_dark'][i%3],2,.17)
        ball((0,0,h*.87),(h*.24,h*.23,h*.13),'leaf',2,.12)


def roof(w,d,z,mat):
    mesh([(-w/2,-d/2,z),(w/2,-d/2,z),(-w/2,d/2,z),(w/2,d/2,z),
          (0,-d/2,z+1.5),(0,d/2,z+1.5)],[(0,4,5,2),(4,1,3,5),(0,1,4),(2,5,3)],mat)
    for x in [-w/2,w/2]:
        rod((x,-d/2,z),(x,d/2,z),.055,'metal')


def wall_with_windows(y,w,z=0,h=3):
    box((0,y,z+.55),(w,.3,1.1),'concrete')
    box((0,y,z+h-.3),(w,.3,.6),'concrete')
    for x in [-w/2+.25,0,w/2-.25]:
        box((x,y,z+h/2),(.5,.3,h),'concrete')


def crenels(radius,z,round_tower=False):
    for i in range(12 if round_tower else 10):
        if round_tower:
            a=i*math.tau/12
            o=box((radius*math.cos(a),radius*math.sin(a),z),(.9,.8,1.3),'stone')
            o.rotation_euler.z=a
        else:
            box((-9+i*2,0,z),(1.1,3,1.2),'stone')


def building(name):
    if name=='house_ruin':
        box((0,0,.12),(8,6,.24),'concrete')
        wall_with_windows(2.85,8)
        wall_with_windows(-2.85,8)
        box((-3.85,0,1.7),(.3,6,3.4),'concrete')
        for y,h in [(-2.3,2.6),(-1.5,2),(1.5,1.4),(2.3,3)]:
            box((3.85,y,h/2),(.3,.8,h),'concrete')
        for i in range(14):
            ball((random.uniform(-3,3),random.uniform(-2,2),.18),(.4,.3,.2),'stone',1,.3)
    elif name=='house_shack':
        for x in [-3,3]:
            for y in [-2.5,2.5]:
                rod((x,y,0),(x,y,4),.14,'wood')
        box((0,0,1.2),(6.6,5.6,.22),'wood')
        for i in range(25):
            x=-3.1+i*.26
            box((x,-2.5,2.35),(.24,.12,2.3),'wood')
            if abs(x)> .7:
                box((x,2.5,2.35),(.24,.12,2.3),'bark')
        for x in [-3.2,3.2]:
            box((x,0,2.35),(.13,5,2.3),'wood')
        roof(7.2,6.4,3.6,'rust')
        for i in range(5):
            box((0,3.5-i*.2,.12+i*.23),(1.5,.45,.22),'wood')
    elif name=='bunker':
        box((0,0,.15),(6,5,.3),'concrete')
        box((0,0,2.7),(6.3,5.3,.7),'concrete')
        box((0,2.25,1.3),(6,.5,2.6),'concrete')
        for x in [-2.75,2.75]:
            box((x,0,1.3),(.5,5,2.6),'concrete')
        box((0,-2.25,.65),(6,.5,1.3),'concrete')
        box((0,-2.25,2.25),(6,.5,.3),'concrete')
        for x in [-2.5,2.5]:
            box((x,-2.25,1.7),(1,.5,.8),'concrete')
    elif name=='watchtower':
        for x in [-1.8,1.8]:
            for y in [-1.8,1.8]:
                rod((x,y,0),(x*.85,y*.85,8.3),.16,'wood')
        for y in [-1.8,1.8]:
            rod((-1.8,y,.5),(1.6,y,6.5),.09,'wood')
            rod((1.8,y,.5),(-1.6,y,6.5),.09,'wood')
        box((0,0,6.4),(4.5,4.5,.25),'wood')
        for z in [6.8,7.3]:
            for y in [-2,2]:
                box((0,y,z),(4,.12,.14),'wood')
        for x in [-.55,.55]:
            rod((x,2.4,0),(x,1.8,6.5),.07,'wood')
        for i in range(17):
            rod((-.55,2.4-i*.035,i*.38),(.55,2.4-i*.035,i*.38),.05,'wood')
        roof(5,5,7.5,'rust')
    elif name=='barracks':
        box((0,0,.15),(6,12,.3),'wood')
        for x in [-3,3]:
            box((x,0,1.3),(.12,12,2.6),'olive')
        roof(6,12,2.6,'olive')
        for y in [-6,6]:
            for x in [-2,2]:
                box((x,y,1.3),(2,.12,2.6),'camo')
            box((0,y,2.5),(2,.12,.3),'camo')
        for y in [-5,-3,-1,1,3,5]:
            for x in [-3.1,3.1]:
                rod((x,y,0),(x,y,2.7),.045,'metal')
                rod((x,y,2.7),(0,y,4.1),.045,'metal')
    elif name=='radio_tower':
        box((3,0,1.5),(3,4,3),'concrete')
        box((3,-2.05,1.1),(1.2,.12,2.2),'metal')
        for i in range(10):
            z=i*2.5
            r=1.8*(1-z/30)
            nr=1.8*(1-(z+2.5)/30)
            for k in range(3):
                a=k*math.tau/3
                b=(k+1)*math.tau/3
                p=(r*math.cos(a),r*math.sin(a),z)
                q=(nr*math.cos(a),nr*math.sin(a),z+2.5)
                rod(p,q,.07,'metal',6)
                rod(p,(nr*math.cos(b),nr*math.sin(b),z+2.5),.035,'rust',6)
                rod(p,(r*math.cos(b),r*math.sin(b),z),.045,'metal',6)
        rod((0,0,24),(0,0,25),.06,'metal')
        ball((0,0,24.5),(.15,.15,.15),'red')
        rod((0,0,18),(2,0,18),.12,'metal')
        dish=ball((2,0,18),(1,.22,1),'white',3)
        rod((2,-.2,18),(2,-1.2,18),.06,'metal')
    elif name=='bridge':
        box((0,0,3.65),(7,40,.7),'metal')
        for i in range(80):
            box((0,-19.75+i*.5,3.96),(6.8,.47,.08),'wood',0)
        for y in [-16,0,16]:
            for x in [-2.6,2.6]:
                box((x,y,-.15),(1,1.4,7.7),'concrete')
        for x in [-3.4,3.4]:
            for z in [4.6,5.3]:
                rod((x,-20,z),(x,20,z),.09,'metal')
            for y in range(-20,21,4):
                rod((x,y,4),(x,y,5.5),.08,'metal')
                if y<20:
                    rod((x,y,4),(x,y+4,5.3),.06,'rust')
    elif name=='fortress_wall':
        box((0,0,3.7),(20,3,7.4),'stone')
        for x in range(-10,11,4):
            box((x,0,3),(1,3.8,6),'concrete')
        box((0,0,7.2),(20.4,3.5,.5),'concrete')
        crenels(0,7.4)
        for z in [1.5,3,4.5,6]:
            box((0,-1.51,z),(20,.025,.07),'bark',0)
    elif name=='fortress_tower':
        rod((0,0,0),(0,0,14),4,'stone',32,3.6)
        rod((0,0,13.5),(0,0,14.5),4.1,'concrete',32)
        crenels(3.6,15,True)
        for a in [0,math.pi/2,math.pi,math.pi*1.5]:
            o=box((3.85*math.cos(a),3.85*math.sin(a),9),(.12,.5,1.7),'black')
            o.rotation_euler.z=a
    elif name=='citadel_keep':
        box((0,0,1),(30,30,2),'stone')
        box((0,3,10),(26,22,18),'stone')
        for x in [-11,11]:
            box((x,-10,10),(8,10,18),'stone')
        box((0,-10,16),(14,10,6),'concrete')
        box((0,-9,5),(7,.3,8),'wood')
        for x in [-12,12]:
            for y in [-12,12]:
                rod((x,y,0),(x,y,21),3.1,'stone',16)
                for a in range(8):
                    t=a*math.tau/8
                    box((x+2.6*math.cos(t),y+2.6*math.sin(t),21.4),(1,1,1.2),'concrete')
        for x in [-8,8]:
            box((x,-15.12,12),(2,.12,5),'red')
            box((x,-15.2,12),(.25,.05,3),'sand')
        for x in range(-8,9,4):
            box((x,-8.1,12),(1,.1,2),'black')
        for x in range(-9,10,3):
            for y in [-14,14]:
                box((x,y,19.6),(1.5,1,1.2),'stone')


def rifle(offset=(0,0,0), scale=1):
    start=len(parts)
    box((0,.12,.12),(.105,.33,.15),'metal',.008)
    box((0,.36,.13),(.10,.24,.12),'black',.009)
    rod((0,.44,.14),(0,.79,.14),.023,'metal',12)
    rod((0,.78,.14),(0,.84,.14),.036,'metal',12)
    for y in [.79,.81,.83]:
        box((0,y,.17),(.04,.012,.015),'black',.002)
    box((0,-.18,.12),(.08,.23,.15),'black',.009)
    box((0,-.30,.105),(.10,.04,.19),'black',.008)
    o=box((0,.16,-.06),(.075,.13,.24),'olive',.012)
    o.rotation_euler.x=-.15
    o=box((0,0,-.045),(.065,.07,.17),'black',.009)
    o.rotation_euler.x=-.25
    for y in [i*.026+.25 for i in range(9)]:
        box((0,y,.20),(.12,.014,.025),'metal',.002)
    box((0,.12,.25),(.07,.10,.08),'black',.005)
    rod((0,.07,.27),(0,.19,.27),.038,'metal',12)
    rod((0,.191,.27),(0,.195,.27),.027,'glass',12)
    for o in parts[start:]:
        o.location=Vector(offset)+o.location*scale
        o.scale*=scale


def prop(name):
    if name=='sandbags':
        for z in range(4):
            for i in range(8):
                a=-1.1+(i+z%2*.4)*.30
                o=ball((math.sin(a)*1.6,math.cos(a)*1.6-.9,.16+z*.28),(.35,.22,.16),'sand',2)
                o.rotation_euler.z=-a
    elif name in ['crate','ammo_box','medkit','intel_case']:
        if name=='intel_case':
            box((0,0,.75),(1.2,.65,.10),'wood')
            for x in [-.5,.5]:
                for y in [-.25,.25]:
                    rod((x,y,0),(x,y,.75),.035,'metal')
            box((0,0,.84),(.55,.38,.08),'black')
            o=box((0,.16,1.03),(.55,.035,.35),'metal'); o.rotation_euler.x=-.16
            box((0,.135,1.04),(.47,.01,.26),'glass',.002)
            return
        size={'crate':(1.2,1.2,1.2),'ammo_box':(.6,.34,.32),'medkit':(.5,.18,.38)}[name]
        w,d,h=size
        mat={'crate':'olive','ammo_box':'olive','medkit':'white'}[name]
        box((0,0,h/2),size,mat)
        box((0,0,h*.92),(w*1.04,d*1.04,h*.08),'metal' if name!='medkit' else 'white')
        for x in [-w*.33,w*.33]:
            box((x,0,h/2),(w*.09,d*1.03,h*1.01),'wood' if name=='crate' else 'metal')
        if name=='medkit':
            box((0,-d*.52,h*.5),(.22,.015,.065),'red',0)
            box((0,-d*.53,h*.5),(.065,.015,.22),'red',0)
        else:
            box((0,-d*.51,h*.55),(w*.35,.015,h*.23),'sand',0)
    elif name=='barrel':
        rod((0,0,0),(0,0,.92),.31,'olive',24)
        for z in [.05,.25,.68,.88]:
            rod((0,0,z),(0,0,z+.035),.325,'metal',24)
        rod((.12,0,.92),(.12,0,.94),.045,'black',12)
    elif name=='flag_pole':
        rod((0,0,0),(0,0,.2),.4,'concrete')
        rod((0,0,0),(0,0,6),.045,'metal')
        ball((0,0,6),(.09,.09,.09),'sand')
        verts=[(i*.35,.10*math.sin(i*1.7),z) for z in [4.2,5.8] for i in range(7)]
        mesh(verts,[(i,i+1,i+8,i+7) for i in range(6)]+[(i+7,i+8,i+1,i) for i in range(6)],'red')
    elif name=='jeep_wreck':
        box((0,0,.65),(1.75,3.5,.35),'rust')
        box((0,1.05,1.05),(1.65,1.25,.45),'black')
        box((0,-1.4,1.1),(1.7,.4,.6),'olive')
        for x in [-.85,.85]:
            box((x,-.3,1),(.12,2,.65),'rust')
            for y in [-1.1,1.1]:
                rod((x-.13,y,.44),(x+.13,y,.44),.44,'black',16)
                rod((x-.14,y,.44),(x+.14,y,.44),.24,'rust',12)
            rod((x,.5,1.2),(x,.7,2),.04,'metal')
        rod((-.85,.7,2),(.85,.7,2),.04,'metal')
        for x in [-.45,.45]:
            box((x,-.35,1.1),(.6,.65,.2),'black')
            box((x,-.65,1.4),(.6,.15,.55),'black')
    elif name=='soldier':
        for x in [-.13,.13]:
            box((x,.065,.085),(.19,.34,.17),'black')
            rod((x,0,.15),(x,-.025,.57),.095,'camo',10)
            rod((x,-.025,.57),(x,0,.96),.115,'olive',10)
            ball((x,-.07,.56),(.105,.10,.11),'black')
        ball((0,0,1.13),(.25,.15,.34),'camo')
        box((0,.025,1.23),(.43,.33,.35),'olive')
        for x in [-.14,0,.14]:
            box((x,.205,1.16),(.11,.08,.16),'camo',.008)
        rod((0,0,1.42),(0,0,1.54),.07,'skin')
        ball((0,0,1.64),(.125,.115,.16),'skin')
        ball((0,-.015,1.73),(.155,.15,.105),'olive')
        box((0,.111,1.67),(.21,.04,.055),'black',.01)
        for sign in [-1,1]:
            rod((sign*.25,0,1.37),(sign*.30,.13,1.12),.085,'camo')
            rod((sign*.30,.13,1.12),(sign*.10,.40,1.22),.075,'olive')
            ball((sign*.10,.40,1.22),(.065,.075,.065),'skin')
        for i in range(18):
            x=random.choice([-1,1])*random.uniform(.07,.19)
            ball((x,.173,random.uniform(1.05,1.4)),(.04,.016,.025),'bark',1)
        rifle((.05,.22,1.15),.72)
    elif name=='drone':
        ball((0,0,.28),(.23,.29,.12),'metal')
        ball((0,.15,.13),(.10,.10,.10),'black')
        ball((0,.24,.13),(.06,.025,.06),'glass')
        for x in [-.43,.43]:
            for y in [-.43,.43]:
                rod((0,0,.28),(x,y,.32),.045,'black')
                rod((x,y,.3),(x,y,.39),.065,'metal')
                box((x,y,.4),(.40,.045,.016),'black',.005)
                rod((x*.6,y*.6,0),(x*.7,y*.7,.28),.025,'metal')
        ball((0,-.28,.29),(.055,.035,.04),'red')


NAMES = ['tree_palm','tree_broadleaf','tree_kapok','bush','fern','rock_a','rock_b',
         'house_ruin','house_shack','bunker','watchtower','barracks','radio_tower',
         'bridge','fortress_wall','fortress_tower','citadel_keep','sandbags','crate',
         'barrel','jeep_wreck','medkit','ammo_box','intel_case','flag_pole','soldier',
         'drone','rifle_viewmodel']
LOD = set(NAMES[:19] + ['soldier','drone'])


def build(name):
    if name.startswith('tree_'):
        tree(name)
    elif name=='bush':
        for i in range(9):
            a=i*2.4
            p=(math.cos(a)*.55,math.sin(a)*.55,.65+random.random()*.35)
            rod((0,0,0),p,.035,'bark')
            ball(p,(.65,.55,.5),'leaf_light' if i%3==0 else 'leaf',2,.18)
    elif name=='fern':
        for i in range(9):
            frond((0,0,.07),i*math.tau/9,.85,.24,.68)
    elif name in ['rock_a','rock_b']:
        for i in range(1 if name=='rock_a' else 5):
            p=(0,0,1.1) if name=='rock_a' else (random.uniform(-1.5,1.5),random.uniform(-1,1),.3+i*.13)
            ball(p,(1.5,1.25,1.1) if name=='rock_a' else (1.25,1.1,.45),'stone' if i%2==0 else 'concrete',3,.20)
    elif name in NAMES[7:17]:
        building(name)
    elif name=='rifle_viewmodel':
        rifle(scale=.8)
    else:
        prop(name)


def bounds(o):
    vs=[o.matrix_world@v.co for v in o.data.vertices]
    return [[min(v[i] for v in vs) for i in range(3)], [max(v[i] for v in vs) for i in range(3)]]


def triangles(o):
    o.data.calc_loop_triangles()
    return len(o.data.loop_triangles)


manifest={}
gallery=[]
for name in NAMES:
    parts=[]
    build(name)
    bpy.ops.object.select_all(action='DESELECT')
    for o in parts:
        o.select_set(True)
    bpy.context.view_layer.objects.active=parts[0]
    bpy.ops.object.join()
    o=bpy.context.object
    o.name=name
    bpy.ops.object.transform_apply(location=False, rotation=True, scale=True)
    o.data.validate()
    b=bounds(o)
    # Ground contact and X/Z centering, except the specified bridge and grip origins.
    if name not in ['bridge','rifle_viewmodel']:
        shift=Vector((-(b[0][0]+b[1][0])/2,-(b[0][1]+b[1][1])/2,-b[0][2]))
        o.location+=shift
    bpy.context.scene.cursor.location=(0,0,0)
    bpy.ops.object.origin_set(type='ORIGIN_CURSOR')
    b=bounds(o)
    engine_bounds=[[b[0][0],b[0][2],-b[1][1]],[b[1][0],b[1][2],-b[0][1]]]
    high=OUT/(name+'.glb')
    bpy.ops.export_scene.gltf(filepath=str(high),export_format='GLB',use_selection=True,export_yup=True)
    low=None
    if name in LOD:
        lo=o.copy(); lo.data=o.data.copy(); bpy.context.collection.objects.link(lo)
        bpy.ops.object.select_all(action='DESELECT'); lo.select_set(True)
        bpy.context.view_layer.objects.active=lo
        dec=lo.modifiers.new('LOD reduction','DECIMATE'); dec.ratio=.22
        bpy.ops.object.modifier_apply(modifier=dec.name)
        lo.data.validate()
        low=OUT/(name+'_low.glb')
        bpy.ops.export_scene.gltf(filepath=str(low),export_format='GLB',use_selection=True,export_yup=True)
        print('LOD',name,triangles(o),triangles(lo),flush=True)
        bpy.data.objects.remove(lo,do_unlink=True)
    manifest[name]={'high':high.relative_to(ROOT).as_posix(),
                    'low':low.relative_to(ROOT).as_posix() if low else None,
                    'bounds':[[round(x,5) for x in row] for row in engine_bounds],
                    'tris':triangles(o)}
    gallery.append(o)
    o.hide_render=True

(OUT/'manifest.json').write_text(json.dumps(manifest,indent=2)+'\n')
total=sum(p.stat().st_size for p in OUT.glob('*.glb'))
assert total<12*1024*1024, total
assert all(manifest[n]['tris']<=6000 for n in NAMES[:3])
assert all(manifest[n]['tris']<=10000 for n in NAMES[7:17])
print('ASSET CHECK OK:',len(manifest),'assets,',total,'GLB bytes',flush=True)

# Individually normalized gallery cells preserve legibility across the size range.
scene=bpy.context.scene
scene.render.engine='BLENDER_WORKBENCH'
scene.display.shading.light='STUDIO'
scene.display.shading.studiolight_rotate_z=.4
scene.display.shading.color_type='MATERIAL'
scene.display.shading.show_shadows=True
scene.display.shading.show_cavity=True
scene.display.shading.cavity_type='BOTH'
scene.display.shading.background_type='WORLD'
scene.world.color=(.07,.085,.10)
for i,o in enumerate(gallery):
    o.hide_render=False
    b=bounds(o)
    size=max(b[1][j]-b[0][j] for j in range(3))
    scale=3.7/size
    o.scale=(scale,)*3
    cell=Vector(((i%7)*5.6,-(i//7)*6.5,0))
    o.location=cell-Vector(((b[0][0]+b[1][0])*scale/2,(b[0][1]+b[1][1])*scale/2,b[0][2]*scale))
    bpy.ops.object.text_add(location=cell+Vector((-2.3,-2.4,0)))
    text=bpy.context.object
    text.data.body=o.name
    text.data.size=.34
    text.data.extrude=.002
    text.data.materials.append(M['white'])
    box(tuple(cell+Vector((0,0,-.16))),(5.2,6,.18),'black',.02)
bpy.ops.object.camera_add(location=(30,-48,52))
camera=bpy.context.object
target=Vector((16.8,-9.8,0))
camera.rotation_euler=(target-camera.location).to_track_quat('-Z','Y').to_euler()
camera.data.type='ORTHO'; camera.data.ortho_scale=46
scene.camera=camera
scene.render.resolution_x=2400
scene.render.resolution_y=1700
scene.render.resolution_percentage=100
(ROOT/'target').mkdir(exist_ok=True)
scene.render.filepath=str(ROOT/'target/owshooter_assets.png')
bpy.ops.render.render(write_still=True)
print('CONTACT SHEET:',scene.render.filepath,flush=True)
