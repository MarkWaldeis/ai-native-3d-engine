"""Deterministic jungle FPS kit. Run with Blender --background --python ... -- OUT [--vegetation-only]."""
import json
import math
from pathlib import Path
import random
import struct
import sys

import bpy
from mathutils import Vector


ROOT = Path(__file__).resolve().parents[2]
args = sys.argv[sys.argv.index('--') + 1:]
OUT = Path(args[0]).resolve()
VEGETATION_ONLY = '--vegetation-only' in args[1:]
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
    ('mortar', (.20,.22,.18), .98,0), ('stone_light', (.42,.43,.35), .96,0),
    ('canvas', (.27,.28,.14), .98,0), ('skin_light', (.57,.39,.25), .85,0),
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


def ball(p, s, mat, sub=2, irregular=0, rng=random):
    bpy.ops.mesh.primitive_ico_sphere_add(subdivisions=sub, radius=1, location=p)
    o = bpy.context.object
    for v in o.data.vertices:
        v.co *= 1 + rng.uniform(-irregular, irregular)
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


def blade(root, tip, lateral, width, oval=False):
    middle = root.lerp(tip, .43)
    ridge = (tip - root).cross(lateral).normalized() * width * .25
    if oval:
        shoulder = root.lerp(tip, .25)
        belly = root.lerp(tip, .64)
        return [root, shoulder - lateral * width * .75, belly - lateral * width, tip,
                belly + lateral * width, shoulder + lateral * width * .75, middle + ridge]
    return [root, middle - lateral * width, tip, middle + lateral * width, middle + ridge]


def blades_mesh(blades, mat):
    verts, faces = [], []
    for blade_verts in blades:
        first = len(verts)
        verts.extend(blade_verts)
        outline = len(blade_verts) - 1
        for i in range(outline):
            faces.append((first + i, first + (i + 1) % outline, first + outline))
    if verts:
        mesh(verts, faces, mat)


def split_frond(base, angle, length, width, rise, mat):
    base = Vector(base)
    forward = Vector((math.cos(angle), math.sin(angle), 0))
    across = Vector((-forward.y, forward.x, 0))
    up = Vector((0, 0, 1))
    segments = 10
    points = [base + forward * length * t + up * (math.sin(t * math.pi) * length * .23 + rise * t)
              for t in [i / segments for i in range(segments + 1)]]
    ribs, faces = [], []
    for i, p in enumerate(points):
        half = max(.002, length * .009 * (1 - i / segments))
        ribs.extend([p - across * half, p + up * half, p + across * half])
        if i:
            k = 3 * (i - 1)
            faces.extend([(k, k + 3, k + 4, k + 1), (k + 1, k + 4, k + 5, k + 2)])
    mesh(ribs, faces, 'leaf_light')
    leaflets = []
    for i, p in enumerate(points[:-1]):
        t = (i + .55) / segments
        reach = width * math.sin(t * math.pi) ** .72
        for sign, stagger in [(-1, .15), (1, .42)]:
            root = p.lerp(points[i + 1], stagger)
            tip = root + across * sign * reach + forward * length * (.14 - .07 * t)
            tip -= up * reach * (.1 + .25 * t)
            leaf_axis = (tip - root).normalized()
            lateral = leaf_axis.cross(up).normalized() * sign
            leaflets.append(blade(root, tip, lateral, .048 * length * (1 - .64 * t)))
    blades_mesh(leaflets, mat)


def canopy(center, size, mat, rng, oval=True):
    center, size = Vector(center), Vector(size)
    ball(center, size * .87, mat, 1, .08, rng)
    blades = []
    count = 10 if oval else 15
    for i in range(count):
        z = 1 - 2 * (i + .5) / count
        angle = i * 2.399963 + rng.uniform(-.16, .16)
        radial = math.sqrt(1 - z * z)
        forward = Vector((math.cos(angle) * radial, math.sin(angle) * radial, z))
        offset = Vector(tuple(forward[j] * size[j] for j in range(3)))
        lateral = forward.cross(Vector((0, 0, 1))).normalized()
        lateral.rotate(forward.rotation_difference(offset.normalized()))
        root = center + offset * .54
        tip = center + offset * rng.uniform(.98, 1.23)
        tip.z -= size.z * rng.uniform(.045, .22)
        width = min(size) * rng.uniform(.15, .27) * (1 if oval else .72)
        blades.append(blade(root, tip, lateral, width, oval))
    blades_mesh(blades, mat)


def hanging_vine(anchor, length, phase):
    anchor = Vector(anchor)
    verts, quads = [], []
    rings, sides = 6, 4
    for i in range(rings + 1):
        t = i / rings
        center = anchor + Vector((math.sin(phase + t * math.tau) - math.sin(phase),
                                  math.cos(phase + t * math.tau) - math.cos(phase), 0)) * length * .04
        center.z -= length * t
        radius = length * (.009 - .007 * t)
        for j in range(sides):
            angle = j * math.tau / sides
            verts.append(center + Vector((math.cos(angle) * radius, math.sin(angle) * radius, 0)))
            if i:
                a = (i - 1) * sides + j
                b = (i - 1) * sides + (j + 1) % sides
                quads.append((a, a + sides, b + sides, b))
    mesh(verts, quads, 'leaf_dark')


def tree(kind, detail=False):
    rng = random.Random(kind) if detail else random
    h = {'tree_palm':10,'tree_broadleaf':14,'tree_kapok':24}[kind]
    if kind == 'tree_palm':
        for i in range(16):
            z = i*h/16
            rod((.5*(z/h)**2,0,z),(.5*((z+h/16)/h)**2,0,z+h/16),
                .24*(1-i/26),'bark',12, .24*(1-(i+1)/26))
            rod((.5*(z/h)**2,0,z),(.5*(z/h)**2,0,z+.06),.245*(1-i/26),'wood',12)
        for i in range(11):
            angle = i * math.tau / 11
            if detail:
                split_frond((.5, 0, h - .5), angle + rng.uniform(-.11, .11), rng.uniform(2.85, 3.5),
                            rng.uniform(.57, .78), rng.uniform(-.72, .16), 'leaf' if i % 3 else 'leaf_light')
            else:
                frond((.5,0,h-.5),angle,3.3,.68, -.6 if i%2 else .1)
        for i in range(5):
            ball((.5+rng.uniform(-.3,.3),rng.uniform(-.3,.3),h-.6),(.16,.16,.20),'bark',rng=rng)
    else:
        r = .65 if h==14 else 1.2
        rod((0,0,0),(0,0,h*.76),r,'bark',14,r*.4)
        for i in range(7):
            a=i*math.tau/7
            x,y=math.cos(a),math.sin(a)
            mesh([(x*r,y*r,0),(x*r*3,y*r*3,0),(x*r,y*r,h*.22),
                  (x*r-y*.3,y*r+x*.3,0)],[(0,1,2),(1,3,2),(3,0,2),(0,3,1)],'bark')
            reach = rng.uniform(.21, .27) if detail else .24
            crown_height = rng.uniform(.74, .86) if detail else .79
            end=(x*h*reach,y*h*reach,h*crown_height)
            rod((0,0,h*.5),end,r*.3,'bark',8,.1)
            mat = ['leaf','leaf_light','leaf_dark'][i%3]
            if detail:
                canopy(end, (h*.24,h*.22,h*(.15 if h==14 else .12)), mat, rng, h==14)
                if h == 24 or i % 2 == 0:
                    for _ in range(2 if h == 24 else 1):
                        anchor = (end[0]+rng.uniform(-.07,.07)*h, end[1]+rng.uniform(-.07,.07)*h, end[2])
                        hanging_vine(anchor, h*rng.uniform(.21,.52), rng.uniform(0, math.tau))
            else: ball(end, (h*.22,h*.21,h*.16), mat, 2, .17)
        if detail: canopy((0,0,h*.87), (h*.24,h*.23,h*.13), 'leaf', rng, h==14)
        else: ball((0,0,h*.87), (h*.24,h*.23,h*.13), 'leaf', 2, .12)


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


def window_frame(x, y, z, w, h, mat='wood', angle=0, door=False):
    start=len(parts)
    box((0,0,z),(w,.025,h),'black',0)
    for dx in [-w/2,w/2]:
        box((dx,-.055,z),(.10,.14,h+.14),mat,.012)
    for dz in [-h/2,h/2]:
        box((0,-.055,z+dz),(w+.18,.14,.10),mat,.012)
    box((0,-.07,z),(.045,.12,h),'metal',.005)
    if door:
        for i in range(7):
            box((-w/2+(i+.5)*w/7,-.026,z),(w/7*.94,.025,h-.12),mat,.006)
        for dz in [-h*.3,h*.3]:
            box((0,-.047,z+dz),(w*.88,.018,.045),'metal',0)
        box((w*.3,-.06,z),(.07,.05,.05),'metal',.007)
    for o in parts[start:]:
        px,py=o.location.x,o.location.y
        o.location.x=x+px*math.cos(angle)-py*math.sin(angle)
        o.location.y=y+px*math.sin(angle)+py*math.cos(angle)
        o.rotation_euler.z+=angle


def roof_ribs(w, d, z):
    for i in range(int(d/.3)+1):
        y=-d/2+.06+i*(d-.12)/int(d/.3)
        for sign in [-1,1]:
            rod((sign*(w/2-.035),y,z+.015),(0,y,z+1.48),.016,'metal',5)
    for sign in [-1,1]:
        box((sign*(w/2-.02),0,z-.025),(.08,d-.06,.11),'wood',.008)


def small_crate(x,y,z):
    box((x,y,z+.32),(.62,.58,.64),'wood',.015)
    for dx in [-.23,.23]:
        box((x+dx,y-.30,z+.32),(.07,.045,.64),'bark',.004)
    for dz in [.10,.54]:
        box((x,y-.31,z+dz),(.62,.05,.065),'metal',.004)


def arch(y, spring, inner, outer):
    for i in range(13):
        a=i*math.pi/13+.012
        b=(i+1)*math.pi/13-.012
        verts=[(r*math.cos(t),yy,spring+r*math.sin(t))
               for yy in [y-.16,y+.28] for r,t in [(inner,a),(outer,a),(outer,b),(inner,b)]]
        mesh(verts,[(0,3,2,1),(4,5,6,7),(0,1,5,4),(1,2,6,5),(2,3,7,6),(3,0,4,7)],
             'stone_light' if i%3 else 'stone')


def masonry_details(name):
    if name=='fortress_wall':
        for side in [-1,1]:
            for row in range(6):
                for col in range(13):
                    x=-9.2+col*1.5+(row%2)*.15
                    box((x,side*1.515,.65+row*1.05),(1.43,.045,.96),
                        'stone_light' if (row+col)%7==0 else 'stone',0)
            for x in [-6,-2,2,6]:
                window_frame(x,side*1.55,5.5,.24,1.25,'concrete',math.pi if side>0 else 0)
        for x in range(-10,11,4):
            box((x,0,.55),(1,3.8,1.1),'stone_light',.04)
        box((0,0,7.49),(19.8,2.5,.12),'mortar',0)
        for x in range(-9,10,2):
            box((x,0,7.57),(1.85,2.35,.06),'stone_light',0)
    elif name=='fortress_tower':
        for z in range(2,14,2):
            r=4-.4*z/14+.035
            rod((0,0,z),(0,0,z+.14),r,'stone_light',32,r-.004)
        for i in range(8):
            a=i*math.tau/8
            for z in [8,11.5]:
                r=4-.4*z/14
                window_frame(r*math.sin(a),-r*math.cos(a),z,.28,1.45,'concrete',a)
            o=box((3.7*math.cos(a),3.7*math.sin(a),2.5),(.65,.6,5),'stone_light',.035)
            o.rotation_euler.z=a
        rod((0,0,14.51),(0,0,14.58),3.5,'mortar',32)
    elif name=='citadel_keep':
        for z in range(3,19,2):
            for x in [-13.025,13.025]:
                box((x,3,z),(.055,21.8,.11),'stone_light',0)
            box((0,14.025,z),(25.9,.055,.11),'stone_light',0)
            for x in [-11,11]:
                box((x,-15.025,z),(7.8,.055,.11),'stone_light',0)
                for dx in [-2,0,2]:
                    box((x+dx+(z%4)*.10,-15.035,z+.5),(.035,.06,.9),'mortar',0)
        for x in [-6,0,6]:
            for z in [7,12,16]:
                window_frame(x,14.025,z,.7,1.6,'concrete',math.pi)
        for x in [-11,11]:
            for z in [7,11,15]:
                window_frame(x,-15.035,z,.65,1.65,'concrete')
        for x in [-5.6,5.6]:
            box((x,-14.85,7.4),(2.6,.5,10.8),'stone_light',.04)
        arch(-14.95,7.4,3.45,4.05)
        for i in range(18):
            x=-3.3+i*.388
            top=7.4+math.sqrt(max(0,3.4**2-x*x))
            box((x,-14.96,(2+top)/2),(.37,.12,top-2),'wood' if i%3 else 'bark',.008)
        for x in [-1.75,1.75]:
            for z in [3.1,5.4,7.2]:
                box((x,-15.06,z),(3.25,.07,.16),'metal',.012)
            for z in [4.7,5.1]:
                box((x*.22,-15.12,z),(.16,.07,.10),'black',.01)
        box((0,3,19.10),(25,21,.14),'mortar',0)
        for x in [-11.6,11.6]:
            box((x,3,19.22),(1.8,20,.10),'stone_light',0)
        box((0,12.5,19.22),(22,1.8,.10),'stone_light',0)
        for x in [-5,5]:
            box((x,-7.9,12),(.5,.5,14),'stone_light',.025)
        verts=[(x,-15.13+.035*math.sin(i*1.4),z)
               for z in [11.3,14.8] for i,x in enumerate([-1,-.5,0,.5,1])]
        mesh(verts,[(i,i+1,i+6,i+5) for i in range(4)]+[(i+5,i+6,i+1,i) for i in range(4)],'red')
        box((0,-15.14,13.4),(.12,.035,1.8),'sand',0)


def outpost_details(name):
    if name in ['house_shack','watchtower','barracks']:
        w,d,z={'house_shack':(7.2,6.4,3.6),'watchtower':(5,5,7.5),'barracks':(6.2,12,2.6)}[name]
        roof_ribs(w,d,z)
    if name=='house_shack':
        for x in [-1.9,1.9]:
            window_frame(x,-2.58,2.55,.8,.85)
        window_frame(0,2.52,2.3,1.15,1.9,'wood',math.pi,door=True)
        for x in [-3,3]:
            rod((x,-2.4,.1),(x,2.4,1.05),.065,'wood',6)
            rod((x,2.4,.1),(x,-2.4,1.05),.065,'wood',6)
        small_crate(2.4,2.55,1.32)
    elif name=='barracks':
        for side in [-1,1]:
            for y in [-4,-1,2,4.5]:
                window_frame(side*3.065,y,1.55,.7,.8,'wood',side*math.pi/2)
            window_frame(0,side*5.95,1.35,1.55,2,'wood',math.pi if side>0 else 0,door=True)
        for i in range(12):
            box((0,-5.5+i, .34),(5.7,.035,.06),'bark',0)
        small_crate(2,5.25,.3)
        for x in [-1.9,1.9]:
            for z in [.5,.78]:
                ball((x,-5.65,z),(.5,.24,.15),'sand',2)
    elif name=='house_ruin':
        for side in [-1,1]:
            for x in [-1.9,1.9]:
                window_frame(x,side*2.91,1.75,1.9,1.05,'wood',math.pi if side>0 else 0)
        for x in [-3.6,3.6]:
            rod((x,-2.75,3.25),(x,2.4,3.25),.09,'wood',6)
        mesh([(-3.7,-2.6,3.2),(-1.4,-2.6,3.35),(-2.5,-.7,3.28),(-3.7,.1,3.2)],[(0,1,2,3)],'rust')
        for i in range(7):
            o=box((-2.8+i*.34,-1.9,.30),(.18,1.8,.08),'wood',.006)
            o.rotation_euler.z=.25
        small_crate(2,1,.25)
    elif name=='watchtower':
        for x in [-1.9,1.9]:
            box((x,0,7.05),(.12,4,.85),'wood',.01)
            for y in [-1.5,-.5,.5,1.5]:
                box((x,y,7.1),(.15,.07,.9),'bark',.005)
        for x in [-1.5,-.75,0,.75,1.5]:
            box((x,0,6.55),(.70,4.1,.08),'wood',.006)
        small_crate(-1,-.9,6.6)
    elif name=='bunker':
        for i in range(7):
            for j in range(6):
                box((-2.64+i*.88,-2.075+j*.83,3.02),(.85,.80,.06),
                    'stone_light' if (i+j)%4==0 else 'concrete',.008)
        for x in [-1.3,1.3]:
            window_frame(x,-2.51,1.7,1.35,.58,'concrete')
        window_frame(0,2.50,1.28,1.25,1.9,'metal',math.pi,door=True)
        for x in [-2.45,-1.85,-1.25,1.25,1.85,2.45]:
            for z in [.48,.75,1.02]:
                ball((x,-2.47,z),(.29,.15,.15),'sand',2)
        for x in [-2.7,2.7]:
            box((x,0,2.26),(.15,4.8,.12),'mortar',.01)


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
        box((0,0,2.68),(6.3,5.3,.66),'concrete')
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


def rifle(offset=None, scale=1):
    viewmodel=offset is None
    if viewmodel:
        offset=(0,0,0)
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
    if viewmodel:
        box((0,.105,.135),(.108,.31,.145),'metal',.012)
        for sign in [-1,1]:
            box((sign*.056,.105,.13),(.012,.25,.085),'black',.005)
            for y in [.015,.075,.135,.195]:
                rod((sign*.061,y,.115),(sign*.066,y,.115),.009,'metal',12)
            for y in [.29,.35,.41,.47]:
                box((sign*.052,y,.14),(.012,.042,.040),'metal',.004)
                box((sign*.060,y,.14),(.006,.030,.024),'black',.003)
            rod((sign*.058,-.20,.12),(sign*.058,-.10,.12),.012,'metal',12)
            for y in [-.285,-.265,-.245]:
                box((sign*.050,y,.105),(.012,.014,.16),'olive',.004)
            for z in [-.13,-.085,-.04]:
                o=box((sign*.040,.158,z),(.012,.105,.012),'black',.003)
                o.rotation_euler.x=-.15
            for z in [-.10,-.06,-.02]:
                o=box((sign*.036,0,z),(.009,.052,.014),'olive',.003)
                o.rotation_euler.x=-.25
            for y in [.315,.375,.435]:
                box((sign*.056,y,.077),(.018,.033,.014),'olive',.003)
        box((.061,.12,.16),(.014,.11,.040),'black',.005)
        box((.070,.09,.16),(.018,.032,.018),'metal',.004)
        rod((-.075,-.025,.19),(.075,-.025,.19),.010,'metal',12)
        box((0,-.04,.18),(.045,.055,.025),'metal',.004)
        for z in [-.12,-.04,.04,.12]:
            box((0,-.323,z),(.085,.012,.012),'metal',.003)
        for y in [.025,.07,.115,.16,.205]:
            box((0,y,.205),(.106,.022,.018),'black',.004)
        for y in [.085,.17]:
            box((0,y,.221),(.095,.024,.025),'metal',.005)
        rod((0,.055,.27),(0,.205,.27),.046,'metal',24)
        for y in [.055,.08,.185,.205]:
            rod((0,y,.27),(0,y+.012,.27),.050,'black',24)
        rod((0,.047,.27),(0,.052,.27),.036,'glass',24)
        rod((0,.217,.27),(0,.222,.27),.036,'glass',24)
        rod((.040,.125,.27),(.062,.125,.27),.020,'black',16)
        rod((0,.125,.309),(0,.125,.328),.018,'black',16)
        rod((0,.50,.14),(0,.77,.14),.028,'black',20)
        for y in [.61,.66,.71,.765,.79,.82]:
            rod((0,y,.14),(0,y+.010,.14),.033,'metal',20)
        for sign in [-1,1]:
            for y in [.795,.818]:
                box((sign*.031,y,.14),(.012,.010,.022),'black',.002)
            rod((sign*.055,.36,.07),(sign*.070,.36,.065),.012,'metal',10)
            rod((sign*.070,.36,.065),(sign*.070,.38,.065),.009,'black',10)
    for o in parts[start:]:
        o.location=Vector(offset)+o.location*scale
        o.scale*=scale


def soldier():
    for x in [-.13,.13]:
        box((x,.065,.025),(.20,.34,.05),'black',.01)
        box((x,.065,.11),(.19,.34,.16),'olive',.03)
        ball((x,.19,.105),(.095,.08,.075),'black',2)
        for y in [-.06,.015,.09,.165]:
            box((x,y,.025),(.205,.025,.025),'metal',0)
            rod((x-.06,y,.19),(x+.06,y+.025,.19),.006,'sand',5)
        rod((x,0,.15),(x,-.025,.57),.095,'camo',16)
        rod((x,-.025,.57),(x,0,.96),.115,'olive',16)
        ball((x,.07,.56),(.105,.10,.11),'black',3)
        box((x,.151,.56),(.135,.045,.15),'olive',.025)
        for z in [.48,.65]:
            box((x,0,z),(.205,.19,.035),'canvas',.008)
    ball((0,.06,1.13),(.24,.15,.30),'camo',3)
    box((0,.10,1.25),(.43,.30,.38),'olive',.035)
    box((0,.267,1.27),(.37,.065,.30),'canvas',.025)
    box((0,-.08,1.24),(.32,.18,.36),'olive',.035)
    box((0,-.15,1.20),(.26,.035,.17),'canvas',.02)
    for x in [-.135,.135]:
        box((x,-.155,1.25),(.035,.025,.29),'black',.005)
        box((x,.10,1.43),(.065,.28,.045),'canvas',.012)
        box((x,.26,1.37),(.05,.035,.06),'black',.006)
    for x in [-.125,0,.125]:
        box((x,.32,1.17),(.105,.095,.19),'camo',.013)
        box((x,.37,1.24),(.105,.025,.055),'olive',.005)
        box((x,.386,1.235),(.022,.008,.035),'black',.002)
    for x in [-.17,-.085,0,.085,.17]:
        box((x,.305,1.35),(.055,.012,.018),'bark',.003)
    box((0,.07,.97),(.46,.32,.065),'black',.012)
    box((0,.24,.97),(.075,.025,.055),'metal',.006)
    box((-.225,.07,1.00),(.07,.14,.17),'canvas',.02)
    ball((.235,.065,1.00),(.07,.08,.12),'olive',3)
    box((.235,.065,1.12),(.06,.055,.035),'black',.006)
    rod((0,.065,1.42),(0,.065,1.56),.07,'skin',16)
    head=ball((0,.065,1.64),(.125,.115,.16),'skin',3)
    for face in head.data.polygons:
        face.use_smooth=True
    for sign in [-1,1]:
        ball((sign*.126,.055,1.64),(.025,.03,.045),'skin_light',2)
        box((sign*.052,.166,1.685),(.064,.025,.018),'bark',.005)
        ball((sign*.052,.174,1.666),(.015,.009,.011),'black',2)
        rod((sign*.13,.065,1.72),(sign*.085,.11,1.54),.012,'canvas',6)
        rod((sign*.085,.11,1.54),(0,.157,1.53),.012,'canvas',6)
    mesh([(-.027,.169,1.67),(.027,.169,1.67),(0,.218,1.625),(-.024,.175,1.61),(.024,.175,1.61)],
         [(0,1,2),(0,2,3),(1,4,2),(2,4,3),(0,3,4,1)],'skin_light')
    box((0,.172,1.585),(.065,.008,.012),'bark',.003)
    verts=[(r*math.cos(i*math.tau/32),.05+r*math.sin(i*math.tau/32),z)
           for r,z in [(.153*math.cos(j*math.pi/12),1.72+.115*math.sin(j*math.pi/12)) for j in range(6)]
           for i in range(32)]
    verts.append((0,.05,1.835))
    faces=[(j*32+i,j*32+(i+1)%32,(j+1)*32+(i+1)%32,(j+1)*32+i) for j in range(5) for i in range(32)]
    faces += [(160+i,160+(i+1)%32,192) for i in range(32)]
    helmet=mesh(verts,faces,'olive')
    for face in helmet.data.polygons:
        face.use_smooth=True
    rod((0,.05,1.708),(0,.05,1.73),.17,'canvas',32,.165)
    for sign in [-1,1]:
        rod((sign*.25,0,1.37),(sign*.30,.13,1.12),.085,'camo',10)
        rod((sign*.30,.13,1.12),(sign*.10,.40,1.22),.075,'olive',16)
        box((sign*.277,.04,1.32),(.13,.17,.13),'canvas',.025)
        ball((sign*.10,.40,1.22),(.065,.075,.065),'black',3)
        for i in range(4):
            x=sign*.10+(i-1.5)*.024
            rod((x,.43,1.23),(x,.485,1.19),.014,'olive',6)
        ball((sign*.15,.40,1.25),(.022,.045,.025),'olive',2)
    for i in range(22):
        x=(-1 if i%2 else 1)*(.13+(i%3-1)*.035)
        z=.27+(i%8)*.09
        box((x,.106 if z>.58 else .083,z),(.045,.012,.026),'bark' if i%3 else 'sand',.004)
    rifle((.05,.22,1.15),.72)
    for y in [.41,.45,.49,.53]:
        for x in [.006,.094]:
            box((x,y,1.245),(.014,.018,.055),'metal',.002)
    box((.05,.35,1.24),(.085,.16,.07),'black',.008)
    box((.10,.31,1.21),(.025,.075,.045),'metal',.004)
    rod((.05,.50,1.30),(.05,.59,1.30),.026,'black',12)
    rod((.05,.59,1.30),(.05,.595,1.30),.021,'glass',12)
    for z in [1.10,1.14,1.18]:
        box((.05,.11,z),(.069,.014,.014),'metal',.002)
    for a,b in [((-.12,.20,1.41),(-.17,.21,1.15)),((-.17,.21,1.15),(.05,.69,1.22))]:
        rod(a,b,.013,'canvas',6)


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
        soldier()
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
VEGETATION = tuple(NAMES[:5])


def build(name, detail=False):
    if name.startswith('tree_'):
        tree(name, detail)
    elif name=='bush':
        rng = random.Random(name) if detail else random
        for i in range(9):
            a=i*2.4
            p=(math.cos(a)*.55,math.sin(a)*.55,.65+rng.random()*.35)
            rod((0,0,0),p,.035,'bark')
            mat = 'leaf_light' if i%3==0 else 'leaf'
            if detail: canopy(p, (.65,.55,.5), mat, rng)
            else: ball(p, (.65,.55,.5), mat, 2, .18)
    elif name=='fern':
        rng = random.Random(name)
        for i in range(9):
            angle = i * math.tau / 9
            if detail:
                split_frond((0, 0, .07), angle + rng.uniform(-.1, .1), rng.uniform(.65, .91),
                            rng.uniform(.18, .26), rng.uniform(.48, .73), 'leaf' if i % 3 else 'leaf_light')
            else:
                frond((0,0,.07),angle,.85,.24,.68)
    elif name in ['rock_a','rock_b']:
        for i in range(1 if name=='rock_a' else 5):
            p=(0,0,1.1) if name=='rock_a' else (random.uniform(-1.5,1.5),random.uniform(-1,1),.3+i*.13)
            ball(p,(1.5,1.25,1.1) if name=='rock_a' else (1.25,1.1,.45),'stone' if i%2==0 else 'concrete',3,.20)
    elif name in NAMES[7:17]:
        building(name)
        masonry_details(name)
        outpost_details(name)
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


def export_glb(path):
    bpy.ops.export_scene.gltf(filepath=str(path),export_format='GLB',use_selection=True,export_yup=True,export_normals=True)
    data=bytearray(path.read_bytes())
    json_size=struct.unpack_from('<I',data,12)[0]
    doc=json.loads(data[20:20+json_size])
    binary=28+json_size
    normals={p['attributes']['NORMAL'] for m in doc['meshes'] for p in m['primitives']}
    for index in normals:
        accessor=doc['accessors'][index]
        assert accessor['componentType']==5126 and accessor['type']=='VEC3'
        view=doc['bufferViews'][accessor['bufferView']]
        start=binary+view.get('byteOffset',0)+accessor.get('byteOffset',0)
        for i in range(accessor['count']):
            offset=start+i*view.get('byteStride',12)
            normal=struct.unpack_from('<3f',data,offset)
            length=math.sqrt(sum(n*n for n in normal))
            assert length>1e-8 and math.isfinite(length), (path,index,i)
            struct.pack_into('<3f',data,offset,*(n/length for n in normal))
    path.write_bytes(data)


def assemble(name, detail=False):
    global parts
    parts=[]
    build(name, detail)
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
    return o


def fit_bounds(o, target_bounds):
    source = bounds(o)
    for vertex in o.data.vertices:
        for i in range(3):
            ratio = (vertex.co[i] - source[0][i]) / (source[1][i] - source[0][i])
            vertex.co[i] = target_bounds[0][i] + ratio * (target_bounds[1][i] - target_bounds[0][i])
    o.data.update()


def export_vegetation(name, entry):
    detailed = assemble(name, detail=True)
    lo, hi = entry['bounds']
    fit_bounds(detailed, [(lo[0], -hi[2], lo[1]), (hi[0], -lo[2], hi[1])])
    export_glb(OUT / (name + '.glb'))
    entry['tris'] = triangles(detailed)
    return detailed


if VEGETATION_ONLY:
    manifest_path = OUT / 'manifest.json'
    manifest = json.loads(manifest_path.read_text())
    for name in VEGETATION:
        bpy.data.objects.remove(export_vegetation(name, manifest[name]), do_unlink=True)
    manifest_path.write_text(json.dumps(manifest, indent=2) + '\n')
    sys.exit(0)


manifest={}
gallery=[]
for name in NAMES:
    o = assemble(name)
    b=bounds(o)
    engine_bounds=[[b[0][0],b[0][2],-b[1][1]],[b[1][0],b[1][2],-b[0][1]]]
    high=OUT/(name+'.glb')
    if name not in VEGETATION:
        export_glb(high)
    low=None
    if name in LOD:
        lo=o.copy(); lo.data=o.data.copy(); bpy.context.collection.objects.link(lo)
        bpy.ops.object.select_all(action='DESELECT'); lo.select_set(True)
        bpy.context.view_layer.objects.active=lo
        dec=lo.modifiers.new('LOD reduction','DECIMATE'); dec.ratio=.22
        bpy.ops.object.modifier_apply(modifier=dec.name)
        lo.data.validate()
        assert .10<=triangles(lo)/triangles(o)<=.25, name
        low=OUT/(name+'_low.glb')
        export_glb(low)
        print('LOD',name,triangles(o),triangles(lo),flush=True)
        bpy.data.objects.remove(lo,do_unlink=True)
    manifest[name]={'high':high.relative_to(ROOT).as_posix(),
                    'low':low.relative_to(ROOT).as_posix() if low else None,
                    'bounds':[[round(x,5) for x in row] for row in engine_bounds],
                    'tris':triangles(o)}
    gallery.append(o)
    o.hide_render=True

for name in VEGETATION:
    index = NAMES.index(name)
    bpy.data.objects.remove(gallery[index], do_unlink=True)
    detailed = export_vegetation(name, manifest[name])
    gallery[index] = detailed
    detailed.hide_render = True

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
