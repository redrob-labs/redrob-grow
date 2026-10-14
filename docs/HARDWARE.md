# Hardware: the office shelf

One steel rack, two tiers, under 1,000,000 KRW. Everything is off the shelf; nothing is soldered.
Prices are late-2026 Korean retail, rounded.

| Part | Why | Approx. KRW |
|---|---|---|
| Steel rack, 2 tiers, 90 cm wide | one tier grows, one holds the gear | 50,000 - 100,000 |
| Deep-water-culture kit (12-24 sites) or NFT channel | lettuce and sprouts | 100,000 - 200,000 |
| Flooded tub 60 x 40 x 20 cm | rice; 15 cm of standing water | 20,000 |
| LED grow light 60-100 W x 2 (200 W class for rice) | the only light the plant can trust | 100,000 - 200,000 |
| Raspberry Pi 5, SD card, PSU | runs Mycodo and `grow-node` | 120,000 - 150,000 |
| SHT31 temperature/humidity (I2C) | air | 10,000 |
| SCD41 CO2 (I2C) | CO2, also air temperature and humidity | 40,000 - 50,000 |
| DS18B20 waterproof (1-Wire) | water temperature | 5,000 |
| EC and pH probes with boards (DFRobot class) | the reservoir | 80,000 - 100,000 |
| Float or ultrasonic level sensor | water level, and a leak guard | 10,000 |
| Aquarium heater 100 W with its own thermostat | water temperature; the thermostat is a second safety | 20,000 |
| Smart plugs x 3-6 (Tasmota or ESPHome firmware, with power metering) | light, heater, pump, fan switched at 220 V with no relay wiring | 15,000 each |
| Air pump + air stone, small circulation pump, USB fan | roots breathe, water moves, lamp heat leaves | 30,000 - 50,000 |
| USB camera | one photo a day is the dataset's eyes | 20,000 - 30,000 |
| Nutrient A/B, pH down, seeds, sponges | consumables | 30,000 |

## Wiring

```
sensors (I2C, 1-Wire) ---> Raspberry Pi ---> Mycodo ---> grow-node ---> dashboard
220 V appliances <--- smart plugs (Wi-Fi) <--- Mycodo (MQTT)
```

Mycodo owns the pins and the plugs. `grow-node` only calls Mycodo's REST API: read the last value
of each Input, switch each Output on **with a duration**. The duration is what makes the system
safe when software dies: Mycodo turns the plug off by itself.

## Safety on a shared shelf

- Water and mains on one rack. Plugs and the Pi go on the upper tier, never above the reservoir.
- The heater's own thermostat is set to the recipe maximum. The guard's 34 C limit is the second line.
- A float switch in the drip tray wired as a Mycodo Input gives the guard a `water_level_pct`
  alarm for leaks.
- Everything on the rack is on one switched power strip a human can reach.

## What is not here

No relay boards, no soldering, no custom PCB, no robot. When the shelf has grown three crops and
the parts that fail are known, that is the time to design a board.
