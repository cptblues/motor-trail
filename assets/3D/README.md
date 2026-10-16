# Motor-Trail — Low-Poly Nature Starter Pack

A first **23-asset 3D prototype** for validating the style of Motor-Trail. Contains real, standalone `.glb` 3D models, the shared visual palette, source generator and a preview contact sheet. All geometry is generated procedurally and is not copied from any third-party pack.

## Contents

- `assets/`: 23 independent GLB files.
- `preview.png`: 2D contact sheet rendered from the actual 3D meshes.
- `catalog.json`: 23 asset metadata records (category, material keys, size in meters, triangle count).
- `generate_assets.py`: deterministic Python generator (requires Python 3, `numpy`, `trimesh`, `Pillow`).
- `MASTER_PROMPT.txt`: master prompt for text-to-3D platforms and example variations.
- `STYLE_GUIDE.md`: art direction and production constraints.

| Category | Assets |
| --- | ---: |
| Trees (broadleaf + conifer) | 7 |
| Rocks | 5 |
| Bushes | 3 |
| Grasses | 3 |
| Ferns | 2 |
| Fallen logs / stump | 3 |
| **Total** | **23** |

## How to use

1. Unzip the pack and inspect `preview.png`.
2. Import individual `assets/*.glb` models into your 3D engine (or Blender).
3. Verify units, normals, world orientation and material colors using one asset before importing everything.
4. Instance environmental decoration objects, and define separate simpler collision meshes for obstacles / trees / rocks according to gameplay.
5. For new AI-generated assets, reuse `MASTER_PROMPT.txt` as the **unchanged base prompt**, altering only the bracketed asset description.

## Prototype status

These files are an **art-direction and pipeline test**, not a polished final production library. They have no authored LOD hierarchy, UV textures, custom gameplay colliders, wind animation, or manual mesh cleanup. Some items may need manual topology, material, silhouette or scale tuning. The prototype has been export/reimport-checked as glTF binary models, but not yet tested in the Motor-Trail game engine.

Assets feature embedded solid-color PBR materials, zero metallic and full roughness. Mesh normals are split per facet to enforce flat shading. Multiple parts with a common material are consolidated into fewer draw calls.
