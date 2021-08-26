-- Create the table that holds the relations between numerous plugin frameworks
CREATE TABLE loaders_relations
(
    parent_loader integer REFERENCES loaders ON DELETE CASCADE,
    child_loader  integer REFERENCES loaders ON DELETE CASCADE,
    CONSTRAINT pkey PRIMARY KEY (parent_loader, child_loader)
);
-- Create a plugin project type
INSERT INTO project_types (id, name)
VALUES (3, 'plugin');
-- Create the initial loaders, in addition to providing sane defaults that are not available on a clean install (fabric + forge)
INSERT INTO loaders (id, loader, icon)
VALUES (1, 'fabric',
        '<svg viewBox="0 0 276 288" fill="none" stroke="currentColor" stroke-width="23" stroke-linecap="round" stroke-linejoin="round"><g transform="matrix(1,0,0,1,-3302.43,-67.3276)"><g transform="matrix(0.564163,0,0,1.70346,1629.87,0)"><g transform="matrix(1.97801,-0.0501803,0.151517,0.655089,1678.7,-354.14)"><g><path d="M820.011,761.092C798.277,738.875 754.809,694.442 734.36,673.389C729.774,668.668 723.992,663.75 708.535,674.369C688.629,688.043 700.073,696.251 703.288,699.785C711.508,708.824 787.411,788.803 800.523,803.818C802.95,806.597 780.243,781.318 793.957,764.065C799.444,757.163 811.985,752.043 820.011,761.092C826.534,768.447 830.658,779.178 816.559,790.826C791.91,811.191 714.618,873.211 689.659,893.792C677.105,904.144 661.053,896.143 653.827,887.719C646.269,878.908 623.211,853.212 602.539,829.646C596.999,823.332 598.393,810.031 604.753,804.545C639.873,774.253 696.704,730.787 716.673,713.831"/></g></g></g></g></svg>');
INSERT INTO loaders (id, loader, icon)
VALUES (2, 'forge',
        '<svg viewBox="0 0 362 208" fill="none" stroke="currentColor" stroke-width="6" stroke-linecap="round" stroke-linejoin="round"><g transform="matrix(1,0,0,1,-3259.27,-486.011)"><g transform="matrix(0.564163,0,0,1.70346,1629.87,0)"><g transform="matrix(6.76583,0,0,2.24074,2829.95,275.109)"><path d="M91.6,16.7L100,14.8L100,7.944L47.452,7.944L47.452,14.388L12,14.1C13.9,15.7 24.4,24.7 31.9,28.4C35.6,30.2 40.2,30.3 44.3,30.4C46.4,30.5 48.5,30.6 50.1,32.2C52.4,34.4 52.9,37.9 50.9,40.5C49,43.1 43.6,43.7 43.6,43.7L39,49.1L39,55.5L85.8,55.5L85.8,49.1L81.3,43.6C81.3,43.6 74.6,43.2 72.9,40.4C67.7,32.6 74.8,20.4 91.6,16.7Z"/></g></g></g></svg>');
INSERT INTO loaders (id, loader, icon)
VALUES (3, 'purpur',
        '<svg width="100%" height="100%" stroke="currentColor" viewBox="0 0 211 343" xmlns="http://www.w3.org/2000/svg" style="fill-rule:evenodd;clip-rule:evenodd;stroke-linejoin:round;fill:none;fill-rule:nonzero;stroke-width:24px;"><g><path d="M92.5,58.912l-80.5,-46.912l0,93.063l80.5,42.368l0,-88.519Z"/><path d="M12,138.563l80.5,42.061l0,91.536l-80.5,-46.605l0,-86.992Z"/><path d="M123.5,162.886l0,-87.932l74.79,41.551l-0,87.932l-74.79,-41.551Z"/><path d="M123.5,288.509l0,-90.623l74.79,41.551l-0,90.623l-74.79,-41.551Z"/></g></svg>')
ON CONFLICT DO NOTHING;
INSERT INTO loaders (id, loader, icon)
VALUES (4, 'spigot',
        '<svg width="100%" height="100%" viewBox="0 0 332 284" version="1.1" xmlns="http://www.w3.org/2000/svg" style="fill-rule:evenodd;clip-rule:evenodd;stroke-linejoin:round;fill:none;fill-rule:nonzero;stroke-width:24px;" stroke="currentColor"><path d="M147.5,27l27,-15l27.5,15l66.5,0l0,33.5l-73,-0.912l0,45.5l26,-0.088l0,31.5l-12.5,0l0,15.5l16,21.5l35,0l0,-21.5l35.5,0l0,21.5l24.5,0l0,55.5l-24.5,0l0,17l-35.5,0l0,-27l-35,0l-55.5,14.5l-67.5,-14.5l-15,14.5l18,12.5l-3,24.5l-41.5,1.5l-48.5,-19.5l6,-19l24.5,-4.5l16,-41l79,-36l-7,-15.5l0,-31.5l23.5,0l0,-45.5l-73.5,0l0,-32.5l67,0Z"/></svg>')
ON CONFLICT DO NOTHING;
INSERT INTO loaders (id, loader, icon)
VALUES (5, 'bukkit',
        '<svg xmlns="http://www.w3.org/2000/svg" width="100%" height="100%" viewBox="0 0 292 319" style="fill-rule:evenodd;clip-rule:evenodd;stroke-linecap:round;stroke-linejoin:round;" stroke="currentColor"><g transform="matrix(1,0,0,1,0,-5)"><path d="M12,109.5L12,155L34.5,224L57.5,224L57.5,271L81,294L160,294L160,172L259.087,172L265,155L265,109.5M12,109.5L12,64L34.5,64L34.5,41L81,17L195.5,17L241,41L241,64L265,64L265,109.5M12,109.5L81,109.5L81,132L195.5,132L195.5,109.5L265,109.5M264.087,204L264.087,244M207.5,272L207.5,312M250,272L250,312L280,312L280,272L250,272ZM192.5,204L192.5,244L222.5,244L222.5,204L192.5,204Z" style="fill:none;fill-rule:nonzero;stroke-width:24px;"/></g></svg>')
ON CONFLICT DO NOTHING;
INSERT INTO loaders (id, loader, icon)
VALUES (6, 'sponge',
        '<svg width="100%" height="100%" viewBox="0 0 268 313" version="1.1" xmlns="http://www.w3.org/2000/svg" style="fill-rule:evenodd;clip-rule:evenodd;stroke-linecap:round;stroke-linejoin:round;fill:none;fill-rule:nonzero;stroke-width:24px;" stroke="currentColor"><path d="M84.299,35.5c-5.547,-13.776 -19.037,-23.5 -34.799,-23.5c-20.711,0 -37.5,16.789 -37.5,37.5c-0,20.711 16.789,37.5 37.5,37.5c20.711,0 37.5,-16.789 37.5,-37.5c0,-4.949 -0.959,-9.674 -2.701,-14Zm0,0l44.701,-8.5l28,65m0,0l-99,20l-18,47.5l15.5,37l-25,32.5l0,72l222.5,0l2.5,-72l-33.5,-117l-65,-20Zm-60,65l0,15m94,-13.5l0,13.5m-67.5,45l46,0l-12.5,50.5l-14.5,0l-19,-50.5Z"/></svg>')
ON CONFLICT DO NOTHING;
INSERT INTO loaders (id, loader, icon)
VALUES (7, 'paper',
        '<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><circle cx="12" cy="12" r="10"></circle><path d="M9.09 9a3 3 0 0 1 5.83 1c0 2-3 3-3 3"></path><line x1="12" y1="17" x2="12.01" y2="17"></line></svg>')
ON CONFLICT DO NOTHING;
-- 1 -> Mod
-- 2 -> Modpack
-- 3 -> Plugin
INSERT INTO loaders_project_types (joining_loader_id, joining_project_type_id)
VALUES (1, 1);
INSERT INTO loaders_project_types (joining_loader_id, joining_project_type_id)
VALUES (1, 2);
INSERT INTO loaders_project_types (joining_loader_id, joining_project_type_id)
VALUES (2, 1);
INSERT INTO loaders_project_types (joining_loader_id, joining_project_type_id)
VALUES (2, 2);
INSERT INTO loaders_project_types (joining_loader_id, joining_project_type_id)
VALUES (3, 3);
INSERT INTO loaders_project_types (joining_loader_id, joining_project_type_id)
VALUES (4, 3);
INSERT INTO loaders_project_types (joining_loader_id, joining_project_type_id)
VALUES (5, 3);
INSERT INTO loaders_project_types (joining_loader_id, joining_project_type_id)
VALUES (6, 3);
INSERT INTO loaders_project_types (joining_loader_id, joining_project_type_id)
VALUES (7, 3);

-- Add relationships between loaders
INSERT INTO loaders_relations (parent_loader, child_loader)
VALUES (5, 3);
INSERT INTO loaders_relations (parent_loader, child_loader)
VALUES (5, 4);
INSERT INTO loaders_relations (parent_loader, child_loader)
VALUES (5, 7);
INSERT INTO loaders_relations (parent_loader, child_loader)
VALUES (4, 3);
INSERT INTO loaders_relations (parent_loader, child_loader)
VALUES (4, 7);
INSERT INTO loaders_relations (parent_loader, child_loader)
VALUES (7, 3);
