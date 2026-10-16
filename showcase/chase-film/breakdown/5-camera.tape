# Beat 5, camera: the chase camera's frustum and baked path, then through it at t = 0 in clay, the match cut into the chase.
# The clock is never played here, so the last frame is the chase track's first key, pfx shot's frame 0.
stage look
clay on
wireframe 0.5
view side
wait 0.5
view top
orbit 20 0.5
frame hero
orbit 30 0.5
camera chase
shade clay
wait 0.6
