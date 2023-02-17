var router ;
var app ;
let wd = new WikiData() ;
var user = { is_logged_in:false } ;


var ns_cache = {
    cache: {},
    loading: {},
    load_namespaces(wikis,callback) {
        let self = this;
        let to_load = [] ;
        wikis.forEach(function(wiki){
            if ( typeof self.cache[wiki]=='undefined') to_load.push(wiki);
        });
        if ( to_load.length==0 ) return callback();
        console.log(to_load);
        callback();// TODO
    } ,
    get_server_for_wiki(wiki) {
        if ( wiki=="wikidatawiki" ) return "www.wikidata.org";
        if ( wiki=="commonswiki" ) return "commons.wikimedia.org";
        if ( wiki=="specieswiki" ) return "species.wikimedia.org";
        if ( wiki=="metawiki" ) return "meta.wikimedia.org";
        let server = wiki.replace(/wiki$/,".wikipedia.org");
        if (wiki!=server) return server;
        return wiki.replace(/^(.+)(wik.+)$/,"$1.$2.org");
    }

};

function set_user_data(d) {
    user = d.user;
    if ( user==null ) {
        $("#user_greeting").text("");
        $("#login_logout").html("<span tt='login'></span>").attr("href","/auth/login");
    } else {
        user.is_logged_in = true;
        $("#user_greeting").html("<a class='btn btn-outline-default' href='#/user/"+user.id+"'>"+user.username+"</a>");
        $("#login_logout").html("<span tt='logout'></span>").attr("href","/auth/logout");
    }
}

$(document).ready(function(){
    vue_components.toolname = 'gulp' ;
    Promise.all ( [
            vue_components.loadComponents ( ['tool-translate','wd-link','commons-thumbnail',//'autodesc','wikidatamap','wd-date','tool-navbar',
                'vue_components/main_page.html',
                'vue_components/list_page.html',
                'vue_components/update_page.html',
                'vue_components/create_list_page.html',
                'vue_components/create_header_page.html',
                'vue_components/list.html',
                'vue_components/cell.html',
                'vue_components/batch-navigator.html',
                ] ) ,
            new Promise(function(resolve, reject) {
                fetch("/auth/info")
                    .then((response) => response.json())
                    .then((d)=>set_user_data(d))
                    .then(resolve)
            } )
    ] ) .then ( () => {
        current_language = tt.language ;
        wd_link_wd = wd ;
        tt.addILdropdown ( '#tooltranslate_wrapper' ) ;

        const routes = [
          { path: '/', component: MainPage },
          { path: '/list/:list_id', component: ListPage , props:true },
          { path: '/list/:list_id/:initial_revision_id', component: ListPage , props:true },
          { path: '/update/:list_or_new', component: UpdatePage , props:true },
          { path: '/create/list', component: CreateListPage , props:true },
          { path: '/create/header', component: CreateHeaderPage , props:true },
/*
          { path: '/group', component: CatalogGroup , props:true },
          { path: '/group/:key', component: CatalogGroup , props:true },
*/
        ] ;

        router = new VueRouter({routes}) ;
        app = new Vue ( { router } ) .$mount('#app') ;
        setTimeout(function(){
            tt.updateInterface($('body')) ;
        },100);
    } ) ;

} ) ;
