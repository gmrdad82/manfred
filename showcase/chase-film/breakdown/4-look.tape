# Beat 4, look: a street lamp's spot cone, then the render region on the tail lights through the chase camera, rendered inside and clay outside.
stage look
clay on
wireframe 0.4
reveal furniture 0.04 0
select furniture.lamp*
frame furniture.lamps
orbit 20 0.6
wireframe off
camera chase
region 0.3 0.5 0.7 0.64
wait 0.5
key Space
wait 0.4
key Space
region off
shade shaded 0.3
wait 0.3
